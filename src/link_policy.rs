//! Link policy (ADR-0039): which link to dial next, how long to wait, and when
//! to move back to a preferred link. Pure state machine: no I/O, no clock, no
//! randomness. The caller passes a monotonic `now` and a jitter value, so every
//! decision is deterministic and testable.
//!
//! Wired into the dial loops of `tunnel.rs` via `LinkSet` (failback and snapshot are not used
//! yet, hence the dead-code allowance).
#![allow(dead_code)]

use std::time::Duration;

/// Hard cap on links per tunnel (ADR-0039 clause 5).
pub const MAX_LINKS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyConfig {
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    /// A session shorter than this counts as a failure of its link.
    pub stable_after: Duration,
    /// Minimum spacing between failback probes of a better link (>= 15 s).
    pub probe_interval: Duration,
    /// A better link must keep passing probes this long before we move.
    pub failback_hold: Duration,
    /// Off by default: without stream resume a move closes the session.
    pub failback_enabled: bool,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            stable_after: Duration::from_secs(30),
            probe_interval: Duration::from_secs(60),
            failback_hold: Duration::from_secs(60),
            failback_enabled: false,
        }
    }
}

impl PolicyConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.initial_backoff.is_zero() {
            return Err("initial_backoff must be > 0");
        }
        if self.max_backoff < self.initial_backoff {
            return Err("max_backoff must be >= initial_backoff");
        }
        if self.probe_interval < Duration::from_secs(15) {
            return Err("probe_interval must be >= 15 s");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// Connect/handshake timeout, reset, refused.
    Network,
    /// The peer rejected our identity or authorization.
    Auth,
    /// The peer presented a different identity than the tunnel's (ADR-0039 clause 7).
    IdentityMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Dial `link` now. `probe` is true when this is a failback probe and the
    /// current session must stay up meanwhile.
    Dial {
        link: usize,
        probe: bool,
        reason: &'static str,
    },
    /// Nothing may be dialed before `until` (monotonic time).
    Wait {
        until: Duration,
        reason: &'static str,
    },
    /// Keep the current session.
    Stay,
    /// Failback criteria met: close the current session and use `link`.
    Switch { link: usize, reason: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStatus {
    Active,
    Dialing,
    Standby,
    Penalized { until: Duration },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkView {
    pub index: usize,
    pub status: LinkStatus,
    pub failures: u32,
    pub last_failure: Option<FailureKind>,
}

#[derive(Debug, Clone, Default)]
struct LinkState {
    failures: u32,
    penalty_until: Duration,
    last_failure: Option<FailureKind>,
    last_probe: Option<Duration>,
    /// Start of the current unbroken run of successful failback probes.
    probe_ok_since: Option<Duration>,
}

#[derive(Debug)]
pub struct LinkPolicy {
    cfg: PolicyConfig,
    links: Vec<LinkState>,
    active: Option<(usize, Duration)>,
    in_flight: Option<(usize, bool)>,
}

impl LinkPolicy {
    pub fn new(cfg: PolicyConfig, link_count: usize) -> Result<Self, &'static str> {
        cfg.validate()?;
        if link_count == 0 || link_count > MAX_LINKS {
            return Err("link count must be 1..=4");
        }
        Ok(Self {
            cfg,
            links: vec![LinkState::default(); link_count],
            active: None,
            in_flight: None,
        })
    }

    fn penalty(&self, failures: u32, jitter_permille: u32) -> Duration {
        let exp = failures.saturating_sub(1).min(16);
        let base = self
            .cfg
            .initial_backoff
            .saturating_mul(1u32 << exp)
            .min(self.cfg.max_backoff);
        // Up to +20% jitter from the caller; never above the cap.
        let extra = base.mul_f64(f64::from(jitter_permille.min(1000)) / 5000.0);
        (base + extra).min(self.cfg.max_backoff)
    }

    fn in_penalty(&self, i: usize, now: Duration) -> bool {
        self.links[i].penalty_until > now
    }

    /// Settle bookkeeping that depends only on elapsed time.
    fn settle(&mut self, now: Duration) {
        if let Some((a, since)) = self.active {
            if now.saturating_sub(since) >= self.cfg.stable_after {
                self.links[a].failures = 0;
                self.links[a].last_failure = None;
            }
        }
    }

    /// What should the tunnel loop do now?
    pub fn next(&mut self, now: Duration) -> Decision {
        self.settle(now);
        if self.in_flight.is_some() {
            return Decision::Stay;
        }
        match self.active {
            None => self.next_without_session(now),
            Some((a, _)) => self.next_with_session(a, now),
        }
    }

    fn next_without_session(&self, now: Duration) -> Decision {
        if let Some(i) = (0..self.links.len()).find(|&i| !self.in_penalty(i, now)) {
            return Decision::Dial {
                link: i,
                probe: false,
                reason: "highest-priority link out of penalty",
            };
        }
        let until = self
            .links
            .iter()
            .map(|l| l.penalty_until)
            .min()
            .unwrap_or(now);
        Decision::Wait {
            until,
            reason: "all links in penalty",
        }
    }

    fn next_with_session(&mut self, a: usize, now: Duration) -> Decision {
        if !self.cfg.failback_enabled || a == 0 {
            return Decision::Stay;
        }
        for i in 0..a {
            if self.in_penalty(i, now) {
                continue;
            }
            let st = &self.links[i];
            if let Some(since) = st.probe_ok_since {
                let fresh = st
                    .last_probe
                    .is_some_and(|p| now.saturating_sub(p) < self.cfg.probe_interval * 2);
                if fresh && now.saturating_sub(since) >= self.cfg.failback_hold {
                    return Decision::Switch {
                        link: i,
                        reason: "better link passed probes for the hold time",
                    };
                }
            }
            let due = st
                .last_probe
                .is_none_or(|p| now.saturating_sub(p) >= self.cfg.probe_interval);
            if due {
                return Decision::Dial {
                    link: i,
                    probe: true,
                    reason: "failback probe of a better link",
                };
            }
        }
        Decision::Stay
    }

    /// The caller started the dial returned by `next`.
    pub fn dial_started(&mut self, link: usize, probe: bool, now: Duration) {
        if probe {
            self.links[link].last_probe = Some(now);
        }
        self.in_flight = Some((link, probe));
    }

    /// A dial (or probe) failed before an authenticated session existed.
    pub fn dial_failed(
        &mut self,
        link: usize,
        kind: FailureKind,
        now: Duration,
        jitter_permille: u32,
    ) {
        self.in_flight = None;
        let st = &mut self.links[link];
        st.probe_ok_since = None;
        st.failures = st.failures.saturating_add(1);
        st.last_failure = Some(kind);
        let failures = st.failures;
        let pen = match kind {
            // Identity problems will not fix themselves: sit at the cap.
            FailureKind::IdentityMismatch | FailureKind::Auth => self.cfg.max_backoff,
            FailureKind::Network => self.penalty(failures, jitter_permille),
        };
        self.links[link].penalty_until = now + pen;
    }

    /// A failback probe completed a full authenticated handshake.
    pub fn probe_succeeded(&mut self, link: usize, now: Duration) {
        self.in_flight = None;
        self.links[link].probe_ok_since.get_or_insert(now);
    }

    /// An authenticated session is established on `link` (not a probe).
    pub fn session_up(&mut self, link: usize, now: Duration) {
        self.in_flight = None;
        self.active = Some((link, now));
        for l in &mut self.links {
            l.probe_ok_since = None;
            l.last_probe = None;
        }
    }

    /// The active session ended. Short-lived sessions count as failures.
    pub fn session_lost(&mut self, now: Duration, jitter_permille: u32) {
        let Some((a, since)) = self.active.take() else {
            return;
        };
        self.in_flight = None;
        if now.saturating_sub(since) < self.cfg.stable_after {
            let st = &mut self.links[a];
            st.failures = st.failures.saturating_add(1);
            st.last_failure = Some(FailureKind::Network);
            let failures = st.failures;
            self.links[a].penalty_until = now + self.penalty(failures, jitter_permille);
        } else {
            self.links[a].failures = 0;
            self.links[a].last_failure = None;
        }
    }

    pub fn snapshot(&self, now: Duration) -> Vec<LinkView> {
        (0..self.links.len())
            .map(|i| {
                let st = &self.links[i];
                let status = if self.active.is_some_and(|(a, _)| a == i) {
                    LinkStatus::Active
                } else if self.in_flight.is_some_and(|(f, _)| f == i) {
                    LinkStatus::Dialing
                } else if st.penalty_until > now {
                    LinkStatus::Penalized {
                        until: st.penalty_until,
                    }
                } else {
                    LinkStatus::Standby
                };
                LinkView {
                    index: i,
                    status,
                    failures: st.failures,
                    last_failure: st.last_failure,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn policy(links: usize, failback: bool) -> LinkPolicy {
        let cfg = PolicyConfig {
            failback_enabled: failback,
            ..PolicyConfig::default()
        };
        LinkPolicy::new(cfg, links).unwrap()
    }

    #[test]
    fn rejects_bad_link_counts_and_config() {
        assert!(LinkPolicy::new(PolicyConfig::default(), 0).is_err());
        assert!(LinkPolicy::new(PolicyConfig::default(), MAX_LINKS + 1).is_err());
        let bad = PolicyConfig {
            probe_interval: s(5),
            ..PolicyConfig::default()
        };
        assert!(bad.validate().is_err());
        let bad = PolicyConfig {
            max_backoff: s(0),
            ..PolicyConfig::default()
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn starts_with_the_highest_priority_link() {
        let mut p = policy(3, false);
        assert!(matches!(
            p.next(s(0)),
            Decision::Dial {
                link: 0,
                probe: false,
                ..
            }
        ));
    }

    #[test]
    fn failed_link_is_skipped_and_one_attempt_at_a_time() {
        let mut p = policy(2, false);
        p.dial_started(0, false, s(0));
        assert_eq!(p.next(s(0)), Decision::Stay, "one attempt in flight");
        p.dial_failed(0, FailureKind::Network, s(0), 0);
        assert!(matches!(p.next(s(0)), Decision::Dial { link: 1, .. }));
    }

    #[test]
    fn backoff_doubles_and_is_capped() {
        let mut p = policy(1, false);
        let mut now = s(0);
        let mut waits = vec![];
        for _ in 0..9 {
            p.dial_started(0, false, now);
            p.dial_failed(0, FailureKind::Network, now, 0);
            let Decision::Wait { until, .. } = p.next(now) else {
                panic!("expected wait")
            };
            waits.push((until - now).as_secs());
            now = until;
        }
        assert_eq!(waits, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
    }

    #[test]
    fn jitter_never_exceeds_the_cap_or_20_percent() {
        let mut p = policy(1, false);
        p.dial_failed(0, FailureKind::Network, s(0), 1000);
        let Decision::Wait { until, .. } = p.next(s(0)) else {
            panic!()
        };
        assert_eq!(until, Duration::from_millis(1200));
        for _ in 0..10 {
            p.dial_failed(0, FailureKind::Network, s(0), 1000);
        }
        let Decision::Wait { until, .. } = p.next(s(0)) else {
            panic!()
        };
        assert_eq!(until, s(60));
    }

    #[test]
    fn all_links_penalized_waits_for_the_earliest() {
        let mut p = policy(2, false);
        p.dial_failed(0, FailureKind::Network, s(0), 0); // until 1
        p.dial_failed(1, FailureKind::Network, s(0), 0);
        p.dial_failed(1, FailureKind::Network, s(0), 0); // until 2
        assert_eq!(
            p.next(s(0)),
            Decision::Wait {
                until: s(1),
                reason: "all links in penalty"
            }
        );
        assert!(matches!(p.next(s(1)), Decision::Dial { link: 0, .. }));
    }

    #[test]
    fn identity_mismatch_and_auth_failures_sit_at_the_cap() {
        let mut p = policy(2, false);
        p.dial_failed(0, FailureKind::IdentityMismatch, s(0), 0);
        p.dial_failed(1, FailureKind::Auth, s(0), 0);
        assert_eq!(
            p.next(s(0)),
            Decision::Wait {
                until: s(60),
                reason: "all links in penalty"
            }
        );
    }

    #[test]
    fn short_session_counts_as_failure_stable_session_resets() {
        let mut p = policy(1, false);
        p.session_up(0, s(0));
        p.session_lost(s(5), 0); // died young
        assert_eq!(p.snapshot(s(5))[0].failures, 1);
        assert!(matches!(p.next(s(5)), Decision::Wait { .. }));
        p.session_up(0, s(100));
        p.session_lost(s(200), 0); // lived 100 s
        let v = &p.snapshot(s(200))[0];
        assert_eq!(v.failures, 0);
        assert!(matches!(p.next(s(200)), Decision::Dial { link: 0, .. }));
    }

    #[test]
    fn failures_reset_only_after_the_session_proved_stable() {
        let mut p = policy(1, false);
        p.dial_failed(0, FailureKind::Network, s(0), 0);
        p.dial_failed(0, FailureKind::Network, s(1), 0);
        p.session_up(0, s(3));
        let _ = p.next(s(10));
        assert_eq!(p.snapshot(s(10))[0].failures, 2, "not yet stable");
        let _ = p.next(s(40));
        assert_eq!(p.snapshot(s(40))[0].failures, 0);
    }

    #[test]
    fn failback_is_off_by_default_even_with_a_better_link() {
        let mut p = policy(2, false);
        p.dial_failed(0, FailureKind::Network, s(0), 0);
        p.session_up(1, s(2));
        assert_eq!(p.next(s(1000)), Decision::Stay);
    }

    #[test]
    fn failback_needs_probes_that_keep_passing_for_the_hold_time() {
        let mut p = policy(2, true);
        p.dial_failed(0, FailureKind::Network, s(0), 0);
        p.session_up(1, s(2));
        // first probe is due as soon as the penalty on link 0 is over
        assert!(matches!(
            p.next(s(10)),
            Decision::Dial {
                link: 0,
                probe: true,
                ..
            }
        ));
        p.dial_started(0, true, s(10));
        p.probe_succeeded(0, s(11));
        assert_eq!(
            p.next(s(40)),
            Decision::Stay,
            "not due yet, hold not elapsed"
        );
        assert!(matches!(
            p.next(s(70)),
            Decision::Dial {
                link: 0,
                probe: true,
                ..
            }
        ));
        p.dial_started(0, true, s(70));
        p.probe_succeeded(0, s(71)); // keeps the original ok_since (11)
        assert!(matches!(p.next(s(72)), Decision::Switch { link: 0, .. }));
    }

    #[test]
    fn a_failed_probe_resets_the_hold_and_penalizes_the_better_link() {
        let mut p = policy(2, true);
        p.dial_failed(0, FailureKind::Network, s(0), 0);
        p.session_up(1, s(2));
        p.dial_started(0, true, s(10));
        p.probe_succeeded(0, s(11));
        p.dial_started(0, true, s(70));
        p.dial_failed(0, FailureKind::Network, s(71), 0);
        assert!(!matches!(p.next(s(72)), Decision::Switch { .. }));
        // no probes are issued while the link is in penalty (anti-flap)
        assert_eq!(p.next(s(72)), Decision::Stay);
    }

    #[test]
    fn stale_probe_success_does_not_trigger_a_switch() {
        let mut p = policy(2, true);
        p.dial_failed(0, FailureKind::Network, s(0), 0);
        p.session_up(1, s(2));
        p.dial_started(0, true, s(10));
        p.probe_succeeded(0, s(11));
        // no further probe for > 2 intervals: evidence is stale, probe again instead
        assert!(matches!(
            p.next(s(200)),
            Decision::Dial {
                link: 0,
                probe: true,
                ..
            }
        ));
    }

    #[test]
    fn primary_active_never_probes() {
        let mut p = policy(3, true);
        p.session_up(0, s(0));
        assert_eq!(p.next(s(500)), Decision::Stay);
    }

    #[test]
    fn alternating_success_failure_never_flaps() {
        // Link 0 answers one probe, then fails, repeatedly: no Switch may ever occur.
        let mut p = policy(2, true);
        p.dial_failed(0, FailureKind::Network, s(0), 0);
        p.session_up(1, s(2));
        let mut now = s(10);
        for _ in 0..40 {
            match p.next(now) {
                Decision::Switch { .. } => panic!("flapped"),
                Decision::Dial {
                    link: 0,
                    probe: true,
                    ..
                } => {
                    p.dial_started(0, true, now);
                    p.probe_succeeded(0, now);
                    // next probe fails
                    now += s(60);
                    p.dial_started(0, true, now);
                    p.dial_failed(0, FailureKind::Network, now, 0);
                }
                _ => {}
            }
            now += s(30);
        }
    }

    #[test]
    fn snapshot_reports_status_without_inventing_data() {
        let mut p = policy(3, false);
        p.session_up(1, s(0));
        p.dial_failed(0, FailureKind::Network, s(0), 0);
        let v = p.snapshot(s(0));
        assert_eq!(v[1].status, LinkStatus::Active);
        assert_eq!(v[0].status, LinkStatus::Penalized { until: s(1) });
        assert_eq!(v[2].status, LinkStatus::Standby);
        assert_eq!(v[0].last_failure, Some(FailureKind::Network));
        assert_eq!(v[2].last_failure, None);
    }
}
