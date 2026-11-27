//! Adaptive optimisation controller (ADR-0056, TP-4.1 / #238).
//!
//! Pure, deterministic decision logic: `Controller` for one direction, `Adaptation` to feed two of
//! them from a session's own counters, and `queue_delay` for the estimate both act on. The 1 Hz
//! driver, the log line and the scheduler reconfiguration live in `policy_relay`, which is also
//! where the opt-in is enforced: `policy.adaptive` plus a configured profile, and only when the
//! `policy-pack` feature and `BAFT_TUNNEL_POLICY_PACK` enable the pack. Every value it can select
//! is one of the five rows of the ADR-0031 table; the window stays the configured profile's for the
//! session's lifetime, so adaptation never changes the carrier, the class or the default data path.

use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use crate::policy::Direction;
use crate::policy_profile::PolicyProfile;

/// Period of the slow loop: at most one sample per second (ADR-0056 decision 3).
pub(crate) const SAMPLE_PERIOD: Duration = Duration::from_secs(1);
/// Consecutive samples beyond a threshold before anything changes (persistence/debounce).
pub(crate) const PERSISTENCE: u8 = 3;
/// Missed samples that make the signal stale and fail safe to the configured row.
pub(crate) const STALE_SAMPLES: u8 = 3;
/// Step down (smaller scheduler pair) above this tunnel-internal queueing-delay estimate.
pub(crate) const DOWN_THRESHOLD: Duration = Duration::from_millis(50);
/// Step back up only below this estimate; the gap is the hysteresis deadband.
pub(crate) const UP_THRESHOLD: Duration = Duration::from_millis(20);
/// Minimum time in one state before another change (also the minimum gap between changes).
pub(crate) const MIN_DWELL: Duration = Duration::from_secs(30);
/// Rate-limit window and the most changes allowed inside it.
pub(crate) const RATE_WINDOW: Duration = Duration::from_secs(300);
pub(crate) const RATE_LIMIT: usize = 3;
/// Time given to a down-step to show an improvement before it is rolled back.
pub(crate) const VERIFY_WINDOW: Duration = Duration::from_secs(60);
/// Minimum improvement, in percent of the pre-step estimate, that keeps a down-step.
pub(crate) const MIN_IMPROVEMENT: u32 = 10;
/// A row that failed verification is not retried for this long.
pub(crate) const BAD_ROW_COOLDOWN: Duration = Duration::from_secs(600);
/// A guard violation or an overflow suspends adaptation for this long.
pub(crate) const GUARD_SUSPENSION: Duration = Duration::from_secs(300);

/// The fixed ladder, least buffering first: exactly the rows of ADR-0031 decision 15, ordered by
/// their quantum and cap (both strictly increase along the array, pinned by a test).
pub(crate) const LADDER: [PolicyProfile; 5] = [
    PolicyProfile::Trading,
    PolicyProfile::Gaming,
    PolicyProfile::Balanced,
    PolicyProfile::Streaming,
    PolicyProfile::Bulk,
];

/// One sampled observation of the pack's own counters. The delay is derived from the arbiter's
/// queued bytes and delivered rate, so it is an estimate (ADR-0024 provenance), not a wire RTT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Signals {
    /// Tunnel-internal queueing-delay estimate for the direction being adapted.
    pub queue_delay: Duration,
    /// Whether this sample had a delivered-byte rate to derive [`Self::queue_delay`] from. When
    /// it did not, the delay is not an observation and the controller's delay-based rules must
    /// not run on it; the guard and overflow counters are still real and still checked.
    pub estimated: bool,
    /// Cumulative scheduler refusals at sample time.
    pub overflow_refusals: u64,
    /// Cumulative guard violations at sample time: requests the arbiter refused because they
    /// exceeded the configured row's cap (ADR-0056 decision 8).
    pub violations: u64,
}

/// Why the controller changed the scheduler pair. Every change carries one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cause {
    /// The estimate stayed above `DOWN_THRESHOLD` for `PERSISTENCE` samples.
    QueueDelayHigh,
    /// The estimate stayed below `UP_THRESHOLD` for `PERSISTENCE` samples.
    QueueDelayLow,
    /// A down-step did not improve the estimate enough inside the verification window.
    NoImprovement,
    /// The fail-safe wrapper counted an interface violation.
    GuardViolation,
    /// The scheduler refused bytes because a queue was over its cap.
    Overflow,
    /// No fresh sample arrived; the configured row is restored.
    StaleSignals,
}

impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::QueueDelayHigh => "queue_delay_high",
            Self::QueueDelayLow => "queue_delay_low",
            Self::NoImprovement => "no_improvement",
            Self::GuardViolation => "guard_violation",
            Self::Overflow => "overflow",
            Self::StaleSignals => "stale_signals",
        })
    }
}

/// One row change, with the signal that caused it. The driver logs this as a single line; the
/// controller never logs per sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Change {
    pub at: Duration,
    pub from: PolicyProfile,
    pub to: PolicyProfile,
    pub cause: Cause,
    /// The estimate at the sample that triggered the change.
    pub queue_delay: Duration,
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "policy adaptive: {} -> {} cause={} queue_delay_ms={}",
            self.from,
            self.to,
            self.cause,
            self.queue_delay.as_millis()
        )
    }
}

/// A down-step waiting for its verification window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    at: Duration,
    before: PolicyProfile,
    baseline: Duration,
}

/// Slow, bounded, reversible chooser over the accepted scheduler rows (ADR-0056).
///
/// It only ever steps one rung down the ladder under sustained queueing delay, one rung back up
/// when the delay stays low, and never above the configured row. A down-step is verified; a step
/// that does not improve the estimate is reverted and its row is cooled down. Guard signals and
/// stale samples fail safe to the configured row.
#[derive(Debug)]
pub(crate) struct Controller {
    configured: PolicyProfile,
    current: PolicyProfile,
    last_change: Option<Duration>,
    change_times: VecDeque<Duration>,
    down_streak: u8,
    up_streak: u8,
    missing: u8,
    pending: Option<Pending>,
    bad_row: Option<(PolicyProfile, Duration)>,
    suspended_until: Option<Duration>,
    overflow_refusals: u64,
    violations: u64,
}

impl Controller {
    /// Starts at the configured row with nothing to verify: a fresh controller changes nothing.
    pub(crate) fn new(configured: PolicyProfile) -> Self {
        Self {
            configured,
            current: configured,
            last_change: None,
            change_times: VecDeque::new(),
            down_streak: 0,
            up_streak: 0,
            missing: 0,
            pending: None,
            bad_row: None,
            suspended_until: None,
            overflow_refusals: 0,
            violations: 0,
        }
    }

    /// The row the tunnel's `policy.profile` selects: the anchor and the fail-safe target.
    #[cfg(test)]
    pub(crate) fn configured(&self) -> PolicyProfile {
        self.configured
    }

    /// The scheduler row in force. Equals [`Self::configured`] until something is changed.
    #[cfg(test)]
    pub(crate) fn current(&self) -> PolicyProfile {
        self.current
    }

    /// One slow-loop tick. `signals` is `None` when no fresh sample exists; `now` is monotonic
    /// time since the controller started. Returns the change to log, if one happened.
    pub(crate) fn update(&mut self, now: Duration, signals: Option<Signals>) -> Option<Change> {
        let Some(signals) = signals else {
            // A gap breaks the streak: fresh samples must rebuild it in full.
            self.down_streak = 0;
            self.up_streak = 0;
            self.missing = self.missing.saturating_add(1);
            if self.missing >= STALE_SAMPLES && self.current != self.configured {
                // No fresh sample means no estimate to record; the cause names the gap.
                return self.restore(now, Cause::StaleSignals, Duration::ZERO);
            }
            return None;
        };
        self.missing = 0;

        // A guard signal is never adapted around: restore and suspend before anything else.
        let guard = signals.violations > self.violations;
        let overflow = signals.overflow_refusals > self.overflow_refusals;
        self.violations = signals.violations;
        self.overflow_refusals = signals.overflow_refusals;
        if guard {
            return self.fail_safe(now, Cause::GuardViolation, signals.queue_delay);
        }
        if overflow {
            return self.fail_safe(now, Cause::Overflow, signals.queue_delay);
        }

        if let Some(until) = self.suspended_until {
            if now < until {
                return None;
            }
            self.suspended_until = None;
        }

        if !signals.estimated {
            // No rate, no estimate: the delay rules have nothing to act on, so the streaks must
            // rebuild from real samples. A pending verification waits for the next estimate
            // instead of being passed by a value that was never measured.
            self.down_streak = 0;
            self.up_streak = 0;
            return None;
        }

        // Verify a pending down-step before considering another change.
        if let Some(pending) = self.pending {
            if now >= pending.at.saturating_add(VERIFY_WINDOW) {
                let improved = signals.queue_delay.saturating_mul(100)
                    <= pending.baseline.saturating_mul(100 - MIN_IMPROVEMENT);
                self.pending = None;
                if !improved {
                    // Revert this step only: the row before it was already verified.
                    let failed = self.current;
                    self.current = pending.before;
                    self.bad_row = Some((failed, now.saturating_add(BAD_ROW_COOLDOWN)));
                    self.down_streak = 0;
                    self.up_streak = 0;
                    self.note_change(now);
                    return Some(Change {
                        at: now,
                        from: failed,
                        to: pending.before,
                        cause: Cause::NoImprovement,
                        queue_delay: signals.queue_delay,
                    });
                }
            }
        }

        if signals.queue_delay > DOWN_THRESHOLD {
            self.down_streak = self.down_streak.saturating_add(1);
            self.up_streak = 0;
        } else if signals.queue_delay < UP_THRESHOLD {
            self.up_streak = self.up_streak.saturating_add(1);
            self.down_streak = 0;
        } else {
            self.down_streak = 0;
            self.up_streak = 0;
        }

        // One step is verified before the next; the verification window is longer than the dwell.
        if self.pending.is_some() {
            return None;
        }

        if self.down_streak >= PERSISTENCE && self.allowed(now) {
            if let Some(target) = self.down_target(now) {
                let before = self.current;
                self.current = target;
                self.down_streak = 0;
                self.pending = Some(Pending {
                    at: now,
                    before,
                    baseline: signals.queue_delay,
                });
                self.note_change(now);
                return Some(Change {
                    at: now,
                    from: before,
                    to: target,
                    cause: Cause::QueueDelayHigh,
                    queue_delay: signals.queue_delay,
                });
            }
        }

        if self.up_streak >= PERSISTENCE && self.allowed(now) {
            if let Some(target) = self.up_target(now) {
                let before = self.current;
                self.current = target;
                self.up_streak = 0;
                self.note_change(now);
                return Some(Change {
                    at: now,
                    from: before,
                    to: target,
                    cause: Cause::QueueDelayLow,
                    queue_delay: signals.queue_delay,
                });
            }
        }

        None
    }

    /// One rung towards `Trading`, unless that is already the floor or the row is cooling down.
    fn down_target(&self, now: Duration) -> Option<PolicyProfile> {
        let index = ladder_index(self.current);
        let target = *LADDER.get(index.checked_sub(1)?)?;
        if self.is_bad(target, now) {
            return None;
        }
        Some(target)
    }

    /// One rung towards the configured row, never past it, never into a cooling-down row.
    fn up_target(&self, now: Duration) -> Option<PolicyProfile> {
        if self.current == self.configured {
            return None;
        }
        let index = ladder_index(self.current);
        let target = *LADDER.get(index + 1)?;
        if ladder_index(target) > ladder_index(self.configured) || self.is_bad(target, now) {
            return None;
        }
        Some(target)
    }

    fn is_bad(&self, row: PolicyProfile, now: Duration) -> bool {
        matches!(self.bad_row, Some((bad, until)) if bad == row && now < until)
    }

    /// Dwell and rate limit for a voluntary step. Both are enforced; the dwell also guarantees a
    /// voluntary change is never closer than `MIN_DWELL` to the previous one. Forced restores and
    /// verification rollbacks bypass this gate (they must be immediate) but still consume the
    /// budget through [`Self::note_change`].
    fn allowed(&mut self, now: Duration) -> bool {
        if let Some(last) = self.last_change {
            if now.saturating_sub(last) < MIN_DWELL {
                return false;
            }
        }
        while let Some(front) = self.change_times.front() {
            if now.saturating_sub(*front) >= RATE_WINDOW {
                self.change_times.pop_front();
            } else {
                break;
            }
        }
        self.change_times.len() < RATE_LIMIT
    }

    fn note_change(&mut self, now: Duration) {
        self.last_change = Some(now);
        self.change_times.push_back(now);
    }

    /// Returns to the configured row; the protected caller for a verified-bad step and for
    /// stale signals.
    fn restore(&mut self, now: Duration, cause: Cause, queue_delay: Duration) -> Option<Change> {
        let before = self.current;
        self.current = self.configured;
        self.pending = None;
        self.down_streak = 0;
        self.up_streak = 0;
        if before == self.current {
            return None;
        }
        self.note_change(now);
        Some(Change {
            at: now,
            from: before,
            to: self.current,
            cause,
            queue_delay,
        })
    }

    /// Fail-safe: restore the configured row and suspend adaptation for a full guard window. A
    /// failed row's cooldown survives, so a suspension cannot shorten it.
    fn fail_safe(&mut self, now: Duration, cause: Cause, queue_delay: Duration) -> Option<Change> {
        self.suspended_until = Some(now.saturating_add(GUARD_SUSPENSION));
        self.restore(now, cause, queue_delay)
    }
}

fn ladder_index(profile: PolicyProfile) -> usize {
    LADDER
        .iter()
        .position(|row| *row == profile)
        .expect("every accepted profile is on the ladder")
}

/// One direction's raw counters, read once per slow-loop tick. These are the pack's own numbers:
/// no RTT, loss, port, protocol or payload signal enters (ADR-0056 decision 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Counters {
    /// Bytes queued in the direction's arbiter at sample time.
    pub queued_bytes: usize,
    /// Bytes the arbiter granted that the relay's writer has not finished writing (ADR-0056
    /// decision 4, amended 2026-11-27). The arbiter's queue is a rendezvous, so this is where a
    /// blocked write actually waits; `queued_bytes` keeps its own meaning and is not redefined.
    pub in_flight_bytes: usize,
    /// Cumulative bytes the arbiter granted, for the rate over the interval.
    pub granted_bytes: u64,
    /// Cumulative scheduler refusals (a queue hit its cap).
    pub overflow_refusals: u64,
    /// Cumulative guard violations: requests above the row's cap that the arbiter refused.
    pub violations: u64,
}

/// The ADR-0056 decision-4 estimate: queued bytes divided by the rate the arbiter delivered over
/// the interval. With no delivered bytes there is no rate to divide by, so the estimate is zero and
/// the loop does not act on it rather than inventing a starvation signal.
fn queue_delay(queued_bytes: usize, granted_bytes: u64, interval: Duration) -> Duration {
    if granted_bytes == 0 || interval.is_zero() {
        return Duration::ZERO;
    }
    let micros = (queued_bytes as u128)
        .saturating_mul(interval.as_micros())
        .saturating_div(granted_bytes as u128);
    Duration::from_micros(u64::try_from(micros).unwrap_or(u64::MAX))
}

/// One change attributed to the direction whose counters caused it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DirectionChange {
    pub direction: Direction,
    pub change: Change,
}

impl std::fmt::Display for DirectionChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let direction = match self.direction {
            Direction::ToPeer => "to_peer",
            Direction::FromPeer => "from_peer",
        };
        write!(f, "{} direction={}", self.change, direction)
    }
}

/// The per-session sampler: one [`Controller`] per relay direction, fed from that direction's own
/// arbiter counters at the ADR's cadence, plus the estimates it computed for diagnostics. Time is
/// passed in, so tests drive it without a clock or a task.
#[derive(Debug)]
pub(crate) struct Adaptation {
    to_peer: Controller,
    from_peer: Controller,
    previous: [Option<Counters>; 2],
    last_at: Option<Duration>,
    history: Vec<(Direction, Duration)>,
    transcript: Vec<DirectionChange>,
}

impl Adaptation {
    pub(crate) fn new(configured: PolicyProfile) -> Self {
        Self {
            to_peer: Controller::new(configured),
            from_peer: Controller::new(configured),
            previous: [None, None],
            last_at: None,
            history: Vec::new(),
            transcript: Vec::new(),
        }
    }

    /// The row a direction's scheduler pair should use now, for tests and diagnostics.
    #[cfg(test)]
    pub(crate) fn current(&self, direction: Direction) -> PolicyProfile {
        match direction {
            Direction::ToPeer => self.to_peer.current(),
            Direction::FromPeer => self.from_peer.current(),
        }
    }

    /// Estimates computed so far, oldest first, for the evidence record only.
    pub(crate) fn history(&self) -> &[(Direction, Duration)] {
        &self.history
    }

    /// Every change made, oldest first, for the evidence record and tests.
    pub(crate) fn transcript(&self) -> &[DirectionChange] {
        &self.transcript
    }

    /// One tick with the counters each direction reported, or `None` when the arbiter could not
    /// answer (a missed sample, which the controller handles as staleness, never as a zero).
    pub(crate) fn tick(
        &mut self,
        now: Duration,
        to_peer: Option<Counters>,
        from_peer: Option<Counters>,
    ) -> Vec<DirectionChange> {
        let interval = self
            .last_at
            .map_or(SAMPLE_PERIOD, |last| now.saturating_sub(last));
        self.last_at = Some(now);
        let mut changes = Vec::new();
        for (index, direction, counters) in [
            (0, Direction::ToPeer, to_peer),
            (1, Direction::FromPeer, from_peer),
        ] {
            // The rate over the interval, or zero when there is none: a first sample, a reset
            // counter, or a direction that delivered nothing.
            let granted = counters.map_or(0, |counters| match self.previous[index] {
                Some(previous) if counters.granted_bytes >= previous.granted_bytes => {
                    counters.granted_bytes - previous.granted_bytes
                }
                _ => 0,
            });
            self.previous[index] = counters;
            // No delivered bytes means no rate to divide by, so there is no estimate. That is a
            // missed sample (ADR-0056 decision 4), never a measured zero: a zero would advance
            // the low-delay streak and satisfy a pending verification with data that does not
            // exist.
            // No delivered bytes means no rate to divide by, so the delay is not an estimate;
            // the counters are still sampled, because a guard violation or an overflow is real
            // whatever the rate. The tick is never turned into a measured zero.
            let signals = counters.map(|counters| Signals {
                // The amended decision-4 estimate counts both kinds of waiting bytes: those still
                // queued in the arbiter and those already granted but not yet written. `queued_bytes`
                // itself is unchanged; the sum is where the new signal enters.
                queue_delay: queue_delay(
                    counters
                        .queued_bytes
                        .saturating_add(counters.in_flight_bytes),
                    granted,
                    interval,
                ),
                estimated: granted > 0,
                overflow_refusals: counters.overflow_refusals,
                violations: counters.violations,
            });
            if let Some(signals) = signals.filter(|signals| signals.estimated) {
                if self.history.len() >= 4_096 {
                    self.history.drain(..2_048);
                }
                self.history.push((direction, signals.queue_delay));
            }
            let controller = match direction {
                Direction::ToPeer => &mut self.to_peer,
                Direction::FromPeer => &mut self.from_peer,
            };
            if let Some(change) = controller.update(now, signals) {
                let change = DirectionChange { direction, change };
                self.transcript.push(change);
                changes.push(change);
            }
        }
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn sample(delay_ms: u64) -> Option<Signals> {
        Some(Signals {
            queue_delay: Duration::from_millis(delay_ms),
            estimated: true,
            overflow_refusals: 0,
            violations: 0,
        })
    }

    fn step_down(controller: &mut Controller, start: u64, delay_ms: u64) -> Change {
        controller.update(at(start), sample(delay_ms));
        controller.update(at(start + 1), sample(delay_ms));
        controller
            .update(at(start + 2), sample(delay_ms))
            .expect("three consecutive samples above the threshold step one row down")
    }

    #[test]
    fn ladder_is_the_profile_table_in_quantum_order() {
        let quanta: Vec<usize> = LADDER.iter().map(|row| row.quantum_bytes()).collect();
        let caps: Vec<usize> = LADDER.iter().map(|row| row.max_queued_bytes()).collect();
        for pair in quanta.windows(2) {
            assert!(pair[0] < pair[1], "quanta must strictly increase");
        }
        for pair in caps.windows(2) {
            assert!(pair[0] < pair[1], "caps must strictly increase");
        }
        for profile in [
            PolicyProfile::Balanced,
            PolicyProfile::Gaming,
            PolicyProfile::Trading,
            PolicyProfile::Streaming,
            PolicyProfile::Bulk,
        ] {
            assert!(LADDER.contains(&profile));
        }
    }

    #[test]
    fn a_calm_stream_never_changes_anything() {
        let mut controller = Controller::new(PolicyProfile::Balanced);
        for second in 0..120 {
            assert_eq!(controller.update(at(second), sample(5)), None);
        }
        assert_eq!(controller.current(), PolicyProfile::Balanced);
    }

    #[test]
    fn sustained_delay_steps_one_row_down_and_names_the_signal() {
        let mut controller = Controller::new(PolicyProfile::Balanced);
        let change = step_down(&mut controller, 0, 80);
        assert_eq!(change.from, PolicyProfile::Balanced);
        assert_eq!(change.to, PolicyProfile::Gaming);
        assert_eq!(change.cause, Cause::QueueDelayHigh);
        assert_eq!(change.queue_delay, Duration::from_millis(80));
        assert_eq!(controller.current(), PolicyProfile::Gaming);
        assert_eq!(controller.configured(), PolicyProfile::Balanced);
    }

    #[test]
    fn every_change_moves_exactly_one_rung_and_never_above_the_configured_row() {
        let mut controller = Controller::new(PolicyProfile::Bulk);
        let mut previous = controller.current();
        let mut changes = 0;
        for step in 0..8u64 {
            let start = step * 100;
            let Some(change) = controller.update(at(start), sample(200)) else {
                continue;
            };
            let from = ladder_index(change.from);
            let to = ladder_index(change.to);
            assert_eq!(from.abs_diff(to), 1, "one bounded step per change");
            assert_eq!(change.from, previous);
            previous = change.to;
            changes += 1;
        }
        assert!(changes >= 1);
        assert!(ladder_index(controller.current()) <= ladder_index(controller.configured()));
    }

    #[test]
    fn the_floor_row_never_steps_below_itself() {
        let mut controller = Controller::new(PolicyProfile::Trading);
        for second in 0..120 {
            assert_eq!(controller.update(at(second), sample(500)), None);
        }
        assert_eq!(controller.current(), PolicyProfile::Trading);
    }

    #[test]
    fn the_deadband_between_thresholds_never_changes_a_row() {
        let mut controller = Controller::new(PolicyProfile::Bulk);
        for second in 0..120 {
            assert_eq!(controller.update(at(second), sample(30)), None);
        }
        assert_eq!(controller.current(), PolicyProfile::Bulk);
    }

    #[test]
    fn changes_are_never_closer_than_the_dwell() {
        let mut controller = Controller::new(PolicyProfile::Bulk);
        let mut changes = vec![];
        for second in 0..600u64 {
            if let Some(change) = controller.update(at(second), sample(200)) {
                changes.push(change);
            }
        }
        assert!(!changes.is_empty());
        for pair in changes.windows(2) {
            assert!(
                pair[1].at.saturating_sub(pair[0].at) >= MIN_DWELL,
                "changes are never closer than the dwell"
            );
        }
    }

    #[test]
    fn the_rate_limit_allows_three_changes_per_window_and_no_more() {
        let mut controller = Controller::new(PolicyProfile::Bulk);
        // Three verified down-steps, each improving by at least 10% inside its window.
        step_down(&mut controller, 0, 200);
        let second = (62..70)
            .find_map(|second| controller.update(at(second), sample(150)))
            .expect("the second step follows its verification");
        assert_eq!(second.to, PolicyProfile::Balanced);
        let third = (124..135)
            .find_map(|second| controller.update(at(second), sample(100)))
            .expect("the third step follows its verification");
        assert_eq!(third.to, PolicyProfile::Gaming);

        // Three changes sit inside the five-minute window, so the fourth waits for it to slide.
        for second in 186..302 {
            assert_eq!(controller.update(at(second), sample(60)), None);
        }
        let fourth = (302..310)
            .find_map(|second| controller.update(at(second), sample(60)))
            .expect("the rate limit releases the fourth change once the window slides");
        assert_eq!(fourth.to, PolicyProfile::Trading);
    }

    #[test]
    fn a_step_that_does_not_improve_is_reverted_and_its_row_cools_down() {
        let mut controller = Controller::new(PolicyProfile::Balanced);
        let down = step_down(&mut controller, 0, 100);
        assert_eq!(down.to, PolicyProfile::Gaming);

        let revert = controller
            .update(at(62), sample(95))
            .expect("a 5% improvement is below the 10% bound");
        assert_eq!(revert.from, PolicyProfile::Gaming);
        assert_eq!(revert.to, PolicyProfile::Balanced);
        assert_eq!(revert.cause, Cause::NoImprovement);

        for second in 70..120 {
            assert_eq!(controller.update(at(second), sample(400)), None);
        }
        assert_eq!(
            controller.current(),
            PolicyProfile::Balanced,
            "the cooled-down row is not retried inside its cooldown"
        );
    }

    #[test]
    fn a_step_that_improves_is_kept_and_the_next_step_waits_its_turn() {
        let mut controller = Controller::new(PolicyProfile::Balanced);
        step_down(&mut controller, 0, 100);
        assert_eq!(
            controller.update(at(62), sample(60)),
            None,
            "an improvement keeps the step without a new change"
        );
        assert_eq!(controller.current(), PolicyProfile::Gaming);
    }

    #[test]
    fn calm_signals_restore_the_configured_row_and_stop_there() {
        let mut controller = Controller::new(PolicyProfile::Bulk);
        step_down(&mut controller, 0, 200);
        controller.update(at(62), sample(120));
        assert_eq!(controller.current(), PolicyProfile::Streaming);

        let mut restored = false;
        for second in 63..200 {
            if let Some(change) = controller.update(at(second), sample(5)) {
                assert_eq!(change.cause, Cause::QueueDelayLow);
                restored = true;
            }
        }
        assert!(restored);
        assert_eq!(controller.current(), PolicyProfile::Bulk);

        for second in 200..400 {
            assert_eq!(controller.update(at(second), sample(5)), None);
        }
        assert_eq!(controller.current(), PolicyProfile::Bulk);
    }

    #[test]
    fn missing_samples_fail_safe_to_the_configured_row() {
        let mut controller = Controller::new(PolicyProfile::Streaming);
        step_down(&mut controller, 0, 200);
        assert_eq!(controller.current(), PolicyProfile::Balanced);

        assert_eq!(controller.update(at(3), None), None);
        assert_eq!(controller.update(at(4), None), None);
        let stale = controller
            .update(at(5), None)
            .expect("three missed samples restore the configured row");
        assert_eq!(stale.to, PolicyProfile::Streaming);
        assert_eq!(stale.cause, Cause::StaleSignals);
        assert_eq!(controller.current(), PolicyProfile::Streaming);
    }

    #[test]
    fn a_stale_gap_resets_the_persistence_streak() {
        let mut controller = Controller::new(PolicyProfile::Bulk);
        assert_eq!(controller.update(at(0), sample(200)), None);
        assert_eq!(controller.update(at(1), sample(200)), None);
        for second in 2..5 {
            assert_eq!(controller.update(at(second), None), None);
        }
        assert_eq!(
            controller.update(at(5), sample(200)),
            None,
            "fresh samples rebuild the streak from zero after a gap"
        );
        assert_eq!(controller.update(at(6), sample(200)), None);
        let change = controller
            .update(at(7), sample(200))
            .expect("three fresh samples after the gap step down");
        assert_eq!(change.to, PolicyProfile::Streaming);
    }

    #[test]
    fn a_guard_suspension_does_not_shorten_a_failed_row_cooldown() {
        let mut controller = Controller::new(PolicyProfile::Bulk);
        step_down(&mut controller, 0, 200);
        let revert = controller
            .update(at(62), sample(195))
            .expect("a 2.5% improvement is below the 10% bound");
        assert_eq!(revert.to, PolicyProfile::Bulk);

        let violation = Some(Signals {
            queue_delay: Duration::from_millis(200),
            estimated: true,
            overflow_refusals: 0,
            violations: 1,
        });
        assert_eq!(
            controller.update(at(65), violation),
            None,
            "the configured row is already restored"
        );

        for second in 370..655 {
            assert_eq!(
                controller.update(at(second), sample(200)),
                None,
                "the failed row is not retried inside its cooldown"
            );
        }
        assert_eq!(controller.update(at(660), sample(200)), None);
        let retried = controller
            .update(at(663), sample(200))
            .expect("the cooldown, not the suspension, is what still blocks the row");
        assert_eq!(retried.to, PolicyProfile::Streaming);
    }

    #[test]
    fn a_guard_violation_restores_the_configured_row_and_suspends_adaptation() {
        let mut controller = Controller::new(PolicyProfile::Balanced);
        step_down(&mut controller, 0, 200);

        let violation = Some(Signals {
            queue_delay: Duration::from_millis(200),
            estimated: true,
            overflow_refusals: 0,
            violations: 1,
        });
        let fallback = controller
            .update(at(3), violation)
            .expect("guard fail-safe");
        assert_eq!(fallback.to, PolicyProfile::Balanced);
        assert_eq!(fallback.cause, Cause::GuardViolation);
        assert_eq!(
            fallback.queue_delay,
            Duration::from_millis(200),
            "the change record keeps the estimate that triggered it"
        );

        for second in 4..200 {
            assert_eq!(controller.update(at(second), violation), None);
        }
        assert_eq!(controller.current(), PolicyProfile::Balanced);

        let resumed = step_down(&mut controller, 400, 200);
        assert_eq!(resumed.to, PolicyProfile::Gaming);
    }

    #[test]
    fn an_overflow_restores_the_configured_row() {
        let mut controller = Controller::new(PolicyProfile::Balanced);
        step_down(&mut controller, 0, 200);
        let overflow = Some(Signals {
            queue_delay: Duration::from_millis(200),
            estimated: true,
            overflow_refusals: 1,
            violations: 0,
        });
        let fallback = controller
            .update(at(3), overflow)
            .expect("overflow fail-safe");
        assert_eq!(fallback.to, PolicyProfile::Balanced);
        assert_eq!(fallback.cause, Cause::Overflow);
    }

    #[test]
    fn the_log_line_names_the_change_the_cause_and_the_signal() {
        let mut controller = Controller::new(PolicyProfile::Balanced);
        let change = step_down(&mut controller, 0, 80);
        let line = change.to_string();
        assert!(line.starts_with("policy adaptive: balanced/1 -> gaming/1"));
        assert!(line.contains("cause=queue_delay_high"));
        assert!(line.contains("queue_delay_ms=80"));
    }

    fn counters(queued: usize, granted: u64) -> Option<Counters> {
        Some(Counters {
            queued_bytes: queued,
            in_flight_bytes: 0,
            granted_bytes: granted,
            overflow_refusals: 0,
            violations: 0,
        })
    }

    #[test]
    fn the_estimate_is_queued_bytes_over_the_delivered_rate() {
        // 100 KB queued while 2.5 MB/s was delivered over one second: 40 ms.
        assert_eq!(
            queue_delay(100_000, 2_500_000, Duration::from_secs(1)),
            Duration::from_millis(40)
        );
        // Half the rate, same queue: twice the delay.
        assert_eq!(
            queue_delay(100_000, 1_250_000, Duration::from_secs(1)),
            Duration::from_millis(80)
        );
        // No queued bytes is no queueing delay, and no delivered bytes is no rate to divide by.
        assert_eq!(
            queue_delay(0, 1_000, Duration::from_secs(1)),
            Duration::ZERO
        );
        assert_eq!(
            queue_delay(1_000, 0, Duration::from_secs(1)),
            Duration::ZERO
        );
    }

    /// ADR-0056 decision 4, amended 2026-11-27: bytes the arbiter granted but the writer has not
    /// finished writing are waiting bytes too. A direction whose arbiter queue reads zero still has
    /// a non-zero estimate while its write is blocked, and `queued_bytes` keeps its own meaning.
    #[test]
    fn the_estimate_counts_granted_but_unwritten_bytes() {
        let mut adaptation = Adaptation::new(PolicyProfile::Balanced);
        adaptation.tick(at(0), counters(0, 0), counters(0, 0));
        let in_flight = Some(Counters {
            queued_bytes: 0,
            in_flight_bytes: 16_384,
            granted_bytes: 16_384,
            overflow_refusals: 0,
            violations: 0,
        });
        adaptation.tick(at(1), in_flight, counters(0, 0));
        assert_eq!(
            adaptation.history(),
            &[(Direction::ToPeer, Duration::from_secs(1))],
            "one 16 KiB grant held for one second at a 16 KiB/s grant rate is 1 s of waiting"
        );
    }

    #[test]
    fn sustained_wired_samples_step_a_direction_and_leave_the_other_alone() {
        let mut adaptation = Adaptation::new(PolicyProfile::Balanced);
        let mut changes = Vec::new();
        // The first tick has no previous counters, so it has no rate to divide by and computes no
        // estimate; the streak is built by the next three.
        for second in 1..=4 {
            // 150 KB queued at 2.5 MB/s is 60 ms, above the down threshold; the other direction
            // is calm and must not move.
            changes.extend(adaptation.tick(
                at(second),
                counters(150_000, second * 2_500_000),
                counters(0, second * 2_500_000),
            ));
        }
        assert_eq!(changes.len(), 1, "one step, one change");
        assert_eq!(changes[0].direction, Direction::ToPeer);
        assert_eq!(changes[0].change.from, PolicyProfile::Balanced);
        assert_eq!(changes[0].change.to, PolicyProfile::Gaming);
        assert_eq!(adaptation.current(Direction::ToPeer), PolicyProfile::Gaming);
        assert_eq!(
            adaptation.current(Direction::FromPeer),
            PolicyProfile::Balanced
        );
        assert!(changes[0].to_string().contains("direction=to_peer"));
        assert_eq!(
            adaptation.history().len(),
            6,
            "one estimate per direction after the first tick"
        );
        // The sample is the queue (150 KB) over the delivered rate (2.5 MB in 1 s): 60 ms.
        assert_eq!(
            adaptation.history()[0],
            (Direction::ToPeer, Duration::from_millis(60))
        );
    }

    #[test]
    fn a_direction_with_no_delivered_bytes_feeds_no_estimate() {
        let mut adaptation = Adaptation::new(PolicyProfile::Balanced);
        // Step down on three 60 ms samples, so a pending verification and a changed row exist.
        for second in 1..=4 {
            adaptation.tick(
                at(second),
                counters(150_000, second * 2_500_000),
                counters(0, second * 2_500_000),
            );
        }
        assert_eq!(adaptation.current(Direction::ToPeer), PolicyProfile::Gaming);
        let history = adaptation.history().len();
        // The arbiter now reports nothing delivered in either direction: there is no rate, so
        // there is no estimate. A fabricated zero would cancel the pending verification as an
        // improvement (0 ms is under any baseline) and start an up-streak; instead only the
        // counters are read, the delay rules stay silent, and the row is held.
        let flat = counters(0, 4 * 2_500_000);
        let mut changes = Vec::new();
        for second in 5..=7 {
            changes.extend(adaptation.tick(at(second), flat, flat));
        }
        assert!(changes.is_empty(), "a tick with no rate changes nothing");
        assert_eq!(
            adaptation.history().len(),
            history,
            "no estimate is recorded"
        );
        assert_eq!(
            adaptation.current(Direction::ToPeer),
            PolicyProfile::Gaming,
            "the row is held while the delay is unknown"
        );
    }

    #[test]
    fn a_missed_sample_is_not_a_zero_and_three_restore_the_configured_row() {
        let mut adaptation = Adaptation::new(PolicyProfile::Balanced);
        for second in 1..=4 {
            adaptation.tick(
                at(second),
                counters(150_000, second * 2_500_000),
                counters(0, second * 2_500_000),
            );
        }
        assert_eq!(adaptation.current(Direction::ToPeer), PolicyProfile::Gaming);
        // Two gaps only clear the streaks; the third restores the configured row.
        assert!(adaptation.tick(at(5), None, None).is_empty());
        assert!(adaptation.tick(at(6), None, None).is_empty());
        let changes = adaptation.tick(at(7), None, None);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].change.cause, Cause::StaleSignals);
        assert_eq!(changes[0].change.to, PolicyProfile::Balanced);
        assert_eq!(
            adaptation.current(Direction::ToPeer),
            PolicyProfile::Balanced
        );
    }

    #[test]
    fn a_guard_increase_on_the_wire_restores_and_suspends() {
        let mut adaptation = Adaptation::new(PolicyProfile::Streaming);
        for second in 1..=4 {
            adaptation.tick(
                at(second),
                counters(300_000, second * 2_500_000),
                counters(0, second * 2_500_000),
            );
        }
        assert_eq!(
            adaptation.current(Direction::ToPeer),
            PolicyProfile::Balanced
        );
        let guarded = Some(Counters {
            queued_bytes: 300_000,
            in_flight_bytes: 0,
            granted_bytes: 10_000_000,
            overflow_refusals: 0,
            violations: 1,
        });
        let changes = adaptation.tick(at(5), guarded, counters(0, 10_000_000));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].change.cause, Cause::GuardViolation);
        assert_eq!(changes[0].change.from, PolicyProfile::Balanced);
        assert_eq!(changes[0].change.to, PolicyProfile::Streaming);
        // The suspension holds for the guard window: a fresh high estimate changes nothing.
        assert!(
            adaptation
                .tick(
                    at(6),
                    counters(300_000, 12_500_000),
                    counters(0, 12_500_000)
                )
                .is_empty()
        );
        assert_eq!(
            adaptation.current(Direction::ToPeer),
            PolicyProfile::Streaming
        );
    }
}
