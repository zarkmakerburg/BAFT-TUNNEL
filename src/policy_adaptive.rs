//! Adaptive optimisation controller core (ADR-0056, TP-4.1 / #238).
//!
//! Pure, deterministic decision logic for the pack's slow control loop. Nothing samples signals,
//! spawns a task, logs to the tunnel or reconfigures the relay yet: like `policy` at TP-2.0, the
//! module settles the rules and is exercised by tests only, behind the `policy-pack` cargo
//! feature. Every value it can select is one of the five rows of the ADR-0031 table; the window
//! stays the configured profile's for the session's lifetime, so adaptation never changes the
//! carrier, the class or the default data path.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

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
    /// Cumulative scheduler refusals at sample time.
    pub overflow_refusals: u64,
    /// Cumulative `Guarded` interface violations at sample time.
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
    pub(crate) fn configured(&self) -> PolicyProfile {
        self.configured
    }

    /// The scheduler row in force. Equals [`Self::configured`] until something is changed.
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

        // Verify a pending down-step before considering another change.
        if let Some(pending) = self.pending {
            if now >= pending.at.saturating_add(VERIFY_WINDOW) {
                let improved =
                    signals.queue_delay.saturating_mul(100) <= pending.baseline.saturating_mul(90);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn sample(delay_ms: u64) -> Option<Signals> {
        Some(Signals {
            queue_delay: Duration::from_millis(delay_ms),
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
}
