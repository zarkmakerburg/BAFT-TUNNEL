//! Transport health model (ADR-0041). Pure logic, no I/O, no clock: the caller
//! supplies a monotonic `now`. Health describes the quality of one link. It is
//! **not** Integrity (ADR-0028) and must never be merged with it.
//!
//! Wired into the unit (`health.json`, quality unknown until an RTT source exists) and the agent
//! status; the RTT/probe feed is a follow-up, hence the dead-code allowance.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::time::Duration;

/// Where the link is in its lifecycle, as reported by the tunnel loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Connecting,
    Connected,
    Authenticated,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    Connecting,
    /// Transport is up; identity not yet verified. Never implies healthy.
    Connected,
    /// Identity verified, but no fresh quality samples: quality is unknown.
    Authenticated,
    Healthy,
    Degraded,
    Recovering,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Thresholds {
    pub rtt_degraded_ms: f64,
    pub jitter_degraded_ms: f64,
    pub loss_degraded: f64,
    pub loss_unavailable: f64,
    /// Samples older than this make a metric unknown.
    pub stale_after: Duration,
    /// Consecutive good evaluations needed to leave Degraded/Recovering.
    pub recover_after: u32,
    /// Minimum probes in the loss window before the ratio is trusted.
    pub min_probes: usize,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            rtt_degraded_ms: 400.0,
            jitter_degraded_ms: 100.0,
            loss_degraded: 0.05,
            loss_unavailable: 0.5,
            stale_after: Duration::from_secs(45),
            recover_after: 3,
            min_probes: 10,
        }
    }
}

/// Smoothed RTT and jitter from keepalive round trips (EWMA, alpha 1/8 and 1/4).
#[derive(Debug, Clone, Default)]
pub struct RttEstimator {
    srtt_ms: Option<f64>,
    jitter_ms: Option<f64>,
    last_sample: Option<Duration>,
}

impl RttEstimator {
    pub fn record(&mut self, rtt: Duration, now: Duration) {
        let r = rtt.as_secs_f64() * 1000.0;
        match self.srtt_ms {
            None => {
                self.srtt_ms = Some(r);
                self.jitter_ms = Some(r / 2.0);
            }
            Some(s) => {
                let dev = (s - r).abs();
                self.jitter_ms = Some(self.jitter_ms.unwrap_or(dev) * 0.75 + dev * 0.25);
                self.srtt_ms = Some(s * 0.875 + r * 0.125);
            }
        }
        self.last_sample = Some(now);
    }

    /// `None` when there is no sample or the last one is stale.
    pub fn fresh(&self, now: Duration, stale_after: Duration) -> Option<(f64, f64)> {
        let last = self.last_sample?;
        if now.saturating_sub(last) > stale_after {
            return None;
        }
        Some((self.srtt_ms?, self.jitter_ms?))
    }
}

/// Fraction of keepalive probes that got no answer, over the last `cap` probes.
#[derive(Debug, Clone)]
pub struct ProbeWindow {
    results: VecDeque<bool>,
    cap: usize,
}

impl ProbeWindow {
    pub fn new(cap: usize) -> Self {
        Self {
            results: VecDeque::with_capacity(cap),
            cap: cap.max(1),
        }
    }

    pub fn record(&mut self, answered: bool) {
        if self.results.len() == self.cap {
            self.results.pop_front();
        }
        self.results.push_back(answered);
    }

    /// `None` until `min_probes` probes exist: a ratio from 2 probes is not data.
    pub fn loss(&self, min_probes: usize) -> Option<f64> {
        if self.results.len() < min_probes.max(1) {
            return None;
        }
        let lost = self.results.iter().filter(|a| !**a).count();
        Some(lost as f64 / self.results.len() as f64)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HealthReport {
    pub state: HealthState,
    /// Why, in plain words. Empty only for Healthy.
    pub reasons: Vec<String>,
    pub rtt_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub probe_loss: Option<f64>,
}

#[derive(Debug)]
pub struct HealthTracker {
    th: Thresholds,
    state: HealthState,
    good_streak: u32,
}

impl HealthTracker {
    pub fn new(th: Thresholds) -> Self {
        Self {
            th,
            state: HealthState::Connecting,
            good_streak: 0,
        }
    }

    pub fn evaluate(
        &mut self,
        now: Duration,
        phase: Phase,
        rtt: &RttEstimator,
        probes: &ProbeWindow,
        failure: Option<&str>,
    ) -> HealthReport {
        let fresh = rtt.fresh(now, self.th.stale_after);
        let loss = probes.loss(self.th.min_probes);
        let (rtt_ms, jitter_ms) = (fresh.map(|f| f.0), fresh.map(|f| f.1));
        let report = |state, reasons| HealthReport {
            state,
            reasons,
            rtt_ms,
            jitter_ms,
            probe_loss: loss,
        };

        match phase {
            Phase::Down => {
                self.good_streak = 0;
                self.state = HealthState::Unavailable;
                let why = failure.map_or_else(
                    || "link is down".to_string(),
                    |f| format!("link is down: {f}"),
                );
                return report(HealthState::Unavailable, vec![why]);
            }
            Phase::Connecting => {
                self.state = HealthState::Connecting;
                return report(HealthState::Connecting, vec!["dialing".into()]);
            }
            Phase::Connected => {
                self.state = HealthState::Connected;
                return report(
                    HealthState::Connected,
                    vec!["transport up, identity not verified yet".into()],
                );
            }
            Phase::Authenticated => {}
        }

        // Authenticated: quality needs fresh data, otherwise it is unknown.
        let Some((r, j)) = fresh else {
            self.state = HealthState::Authenticated;
            return report(
                HealthState::Authenticated,
                vec!["no fresh quality samples (quality unknown)".into()],
            );
        };

        let mut bad = vec![];
        let mut severe = false;
        if r > self.th.rtt_degraded_ms {
            bad.push(format!(
                "rtt {r:.0} ms above {:.0} ms",
                self.th.rtt_degraded_ms
            ));
        }
        if j > self.th.jitter_degraded_ms {
            bad.push(format!(
                "jitter {j:.0} ms above {:.0} ms",
                self.th.jitter_degraded_ms
            ));
        }
        if let Some(l) = loss {
            if l >= self.th.loss_unavailable {
                severe = true;
                bad.push(format!(
                    "probe loss {:.0}% at or above {:.0}%",
                    l * 100.0,
                    self.th.loss_unavailable * 100.0
                ));
            } else if l > self.th.loss_degraded {
                bad.push(format!(
                    "probe loss {:.0}% above {:.0}%",
                    l * 100.0,
                    self.th.loss_degraded * 100.0
                ));
            }
        }

        if !bad.is_empty() {
            self.good_streak = 0;
            self.state = if severe {
                HealthState::Unavailable
            } else {
                HealthState::Degraded
            };
            return report(self.state, bad);
        }

        // Good evaluation. Leaving a bad state needs a streak (no flapping).
        match self.state {
            HealthState::Degraded | HealthState::Unavailable | HealthState::Recovering => {
                self.good_streak += 1;
                if self.good_streak >= self.th.recover_after {
                    self.state = HealthState::Healthy;
                    self.good_streak = 0;
                    report(HealthState::Healthy, vec![])
                } else {
                    self.state = HealthState::Recovering;
                    report(
                        HealthState::Recovering,
                        vec![format!(
                            "{}/{} good evaluations",
                            self.good_streak, self.th.recover_after
                        )],
                    )
                }
            }
            _ => {
                self.state = HealthState::Healthy;
                report(HealthState::Healthy, vec![])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }
    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }
    fn full_window(answered: usize, lost: usize) -> ProbeWindow {
        let mut w = ProbeWindow::new(20);
        for _ in 0..answered {
            w.record(true);
        }
        for _ in 0..lost {
            w.record(false);
        }
        w
    }
    fn est(rtt_ms: u64, now: Duration) -> RttEstimator {
        let mut e = RttEstimator::default();
        e.record(ms(rtt_ms), now);
        e
    }

    #[test]
    fn connected_is_never_healthy() {
        let mut t = HealthTracker::new(Thresholds::default());
        let r = t.evaluate(
            s(1),
            Phase::Connected,
            &est(10, s(1)),
            &full_window(20, 0),
            None,
        );
        assert_eq!(r.state, HealthState::Connected);
    }

    #[test]
    fn authenticated_without_samples_is_unknown_quality_not_healthy() {
        let mut t = HealthTracker::new(Thresholds::default());
        let r = t.evaluate(
            s(1),
            Phase::Authenticated,
            &RttEstimator::default(),
            &ProbeWindow::new(20),
            None,
        );
        assert_eq!(r.state, HealthState::Authenticated);
        assert_eq!((r.rtt_ms, r.jitter_ms, r.probe_loss), (None, None, None));
    }

    #[test]
    fn stale_samples_become_unknown() {
        let mut t = HealthTracker::new(Thresholds::default());
        let e = est(20, s(0));
        let r = t.evaluate(s(100), Phase::Authenticated, &e, &full_window(20, 0), None);
        assert_eq!(r.state, HealthState::Authenticated);
        assert_eq!(r.rtt_ms, None);
    }

    #[test]
    fn fresh_good_samples_are_healthy_with_no_reasons() {
        let mut t = HealthTracker::new(Thresholds::default());
        let r = t.evaluate(
            s(1),
            Phase::Authenticated,
            &est(30, s(1)),
            &full_window(20, 0),
            None,
        );
        assert_eq!(r.state, HealthState::Healthy);
        assert!(r.reasons.is_empty());
        assert!(r.rtt_ms.is_some());
    }

    #[test]
    fn few_probes_do_not_produce_a_loss_ratio() {
        let w = full_window(1, 1);
        assert_eq!(w.loss(10), None);
    }

    #[test]
    fn high_rtt_or_moderate_loss_is_degraded_with_a_reason() {
        let mut t = HealthTracker::new(Thresholds::default());
        let r = t.evaluate(
            s(1),
            Phase::Authenticated,
            &est(900, s(1)),
            &full_window(20, 0),
            None,
        );
        assert_eq!(r.state, HealthState::Degraded);
        assert!(r.reasons[0].contains("rtt"));
        let mut t = HealthTracker::new(Thresholds::default());
        let r = t.evaluate(
            s(1),
            Phase::Authenticated,
            &est(30, s(1)),
            &full_window(17, 3),
            None,
        );
        assert_eq!(r.state, HealthState::Degraded);
    }

    #[test]
    fn heavy_loss_is_unavailable_even_when_authenticated() {
        let mut t = HealthTracker::new(Thresholds::default());
        let r = t.evaluate(
            s(1),
            Phase::Authenticated,
            &est(30, s(1)),
            &full_window(5, 15),
            None,
        );
        assert_eq!(r.state, HealthState::Unavailable);
    }

    #[test]
    fn recovery_needs_a_streak_of_good_evaluations() {
        let mut t = HealthTracker::new(Thresholds::default());
        let bad = est(900, s(1));
        t.evaluate(s(1), Phase::Authenticated, &bad, &full_window(20, 0), None);
        let good = est(30, s(2));
        let w = full_window(20, 0);
        let r1 = t.evaluate(s(2), Phase::Authenticated, &good, &w, None);
        assert_eq!(r1.state, HealthState::Recovering);
        let r2 = t.evaluate(s(3), Phase::Authenticated, &good, &w, None);
        assert_eq!(r2.state, HealthState::Recovering);
        let r3 = t.evaluate(s(4), Phase::Authenticated, &good, &w, None);
        assert_eq!(r3.state, HealthState::Healthy);
    }

    #[test]
    fn one_bad_sample_during_recovery_restarts_the_streak() {
        let mut t = HealthTracker::new(Thresholds::default());
        let w = full_window(20, 0);
        t.evaluate(s(1), Phase::Authenticated, &est(900, s(1)), &w, None);
        t.evaluate(s(2), Phase::Authenticated, &est(30, s(2)), &w, None);
        t.evaluate(s(3), Phase::Authenticated, &est(900, s(3)), &w, None);
        let r = t.evaluate(s(4), Phase::Authenticated, &est(30, s(4)), &w, None);
        assert_eq!(r.state, HealthState::Recovering);
    }

    #[test]
    fn down_reports_unavailable_with_the_failure_reason() {
        let mut t = HealthTracker::new(Thresholds::default());
        let r = t.evaluate(
            s(1),
            Phase::Down,
            &RttEstimator::default(),
            &ProbeWindow::new(5),
            Some("handshake timeout"),
        );
        assert_eq!(r.state, HealthState::Unavailable);
        assert!(r.reasons[0].contains("handshake timeout"));
    }

    #[test]
    fn after_down_a_reconnected_link_goes_through_recovering() {
        let mut t = HealthTracker::new(Thresholds::default());
        let w = full_window(20, 0);
        t.evaluate(s(1), Phase::Down, &RttEstimator::default(), &w, None);
        let r = t.evaluate(s(2), Phase::Authenticated, &est(30, s(2)), &w, None);
        assert_eq!(r.state, HealthState::Recovering);
    }

    #[test]
    fn rtt_estimator_smooths_and_tracks_jitter() {
        let mut e = RttEstimator::default();
        e.record(ms(100), s(0));
        e.record(ms(100), s(1));
        let (r, j) = e.fresh(s(1), s(45)).unwrap();
        assert!((r - 100.0).abs() < 1e-9);
        assert!(j < 50.0);
        e.record(ms(300), s(2));
        let (r2, j2) = e.fresh(s(2), s(45)).unwrap();
        assert!(r2 > 100.0 && r2 < 300.0);
        assert!(j2 > j);
    }

    #[test]
    fn probe_window_is_bounded() {
        let mut w = ProbeWindow::new(10);
        for _ in 0..100 {
            w.record(false);
        }
        for _ in 0..10 {
            w.record(true);
        }
        assert_eq!(w.loss(10), Some(0.0));
    }
}
