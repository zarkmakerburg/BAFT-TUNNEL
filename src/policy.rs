//! Engine-agnostic traffic policy interface (ADR-0044, TP-2.0 / #245).
//!
//! Compiled only with the `policy-pack` cargo feature (off by default). Nothing in the relay
//! calls it yet: this module defines the versioned interface, the capability declaration and the
//! fail-safe wrapper, with a pass-through Balanced policy as the only implementation. Queues,
//! schedulers and profile parameters are later work (TP-2.1, TP-3.x). No benefit is claimed.
#![allow(dead_code)]

use std::time::Duration;

/// Version of this interface. A profile or engine declaring another version keeps the pack off.
pub const POLICY_INTERFACE_VERSION: u32 = 1;

/// Upper bound on any wait a policy may impose before data moves (bounded wait, ADR-0031 rule 5).
pub const MAX_GRANT_DELAY: Duration = Duration::from_millis(250);

/// Opaque identifier of one relayed flow (one Yamux stream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlowId(pub u64);

/// Traffic class, set only from explicit configuration (no payload inspection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Class {
    #[default]
    Balanced,
    Gaming,
    Trading,
    Streaming,
    Bulk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    ToPeer,
    FromPeer,
}

/// Unit of data a decision applies to. `Datagram` is reserved for the UDP carrier (#244).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Bytes(usize),
    Datagram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flow {
    pub id: FlowId,
    pub class: Class,
    pub direction: Direction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admit {
    Accept,
    Reject(&'static str),
}

/// How much may move now and how long to wait first. `delay` never exceeds `MAX_GRANT_DELAY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant {
    pub bytes: usize,
    pub delay: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Moved(usize),
    QueueDepth(usize),
    Closed,
}

/// A capability is never assumed: the default is `Unknown`, and only `Supported` may back a claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Support {
    Supported,
    Unsupported,
    #[default]
    Unknown,
}

/// Capabilities of one (engine, carrier) tuple (ADR-0031 decisions 12 and 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    pub priority_enforcement: Support,
    pub write_pacing: Support,
    pub rtt_observation: Support,
    pub queue_observation: Support,
    pub datagram: Support,
}

impl Capabilities {
    /// True only when every listed capability is `Supported`; anything else is not "ready".
    pub fn all_supported(&self) -> bool {
        [
            self.priority_enforcement,
            self.write_pacing,
            self.rtt_observation,
            self.queue_observation,
            self.datagram,
        ]
        .iter()
        .all(|s| *s == Support::Supported)
    }
}

/// The policy decision points. Calls are non-blocking and bounded.
pub trait TrafficPolicy: Send + Sync {
    fn interface_version(&self) -> u32 {
        POLICY_INTERFACE_VERSION
    }
    fn admit(&self, flow: &Flow) -> Admit;
    fn grant(&self, flow: &Flow, wanted: Unit) -> Grant;
    fn observe(&self, flow: &Flow, event: Event);
}

/// Balanced pass-through: accepts every flow and grants exactly what was asked, with no delay.
/// This is the fail-safe target and equals the data path with the pack off.
#[derive(Debug, Default, Clone, Copy)]
pub struct Balanced;

impl TrafficPolicy for Balanced {
    fn admit(&self, _flow: &Flow) -> Admit {
        Admit::Accept
    }
    fn grant(&self, _flow: &Flow, wanted: Unit) -> Grant {
        Grant {
            bytes: match wanted {
                Unit::Bytes(n) => n,
                Unit::Datagram => 0,
            },
            delay: Duration::ZERO,
        }
    }
    fn observe(&self, _flow: &Flow, _event: Event) {}
}

/// Wraps a policy and enforces the interface rules: version match, bounded delay, never grant
/// more than asked. A version mismatch or a violation falls back to `Balanced` and counts it.
pub struct Guarded<P: TrafficPolicy> {
    inner: P,
    fallback: Balanced,
    violations: std::sync::atomic::AtomicU64,
}

impl<P: TrafficPolicy> Guarded<P> {
    pub fn new(inner: P) -> Self {
        Self {
            inner,
            fallback: Balanced,
            violations: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn violations(&self) -> u64 {
        self.violations.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn compatible(&self) -> bool {
        self.inner.interface_version() == POLICY_INTERFACE_VERSION
    }

    fn violation(&self) {
        self.violations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl<P: TrafficPolicy> TrafficPolicy for Guarded<P> {
    fn admit(&self, flow: &Flow) -> Admit {
        if self.compatible() {
            self.inner.admit(flow)
        } else {
            self.fallback.admit(flow)
        }
    }

    fn grant(&self, flow: &Flow, wanted: Unit) -> Grant {
        if !self.compatible() {
            return self.fallback.grant(flow, wanted);
        }
        let grant = self.inner.grant(flow, wanted);
        let asked = match wanted {
            Unit::Bytes(n) => n,
            Unit::Datagram => 0,
        };
        if grant.bytes > asked || grant.delay > MAX_GRANT_DELAY {
            self.violation();
            return self.fallback.grant(flow, wanted);
        }
        grant
    }

    fn observe(&self, flow: &Flow, event: Event) {
        if self.compatible() {
            self.inner.observe(flow, event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow() -> Flow {
        Flow {
            id: FlowId(1),
            class: Class::default(),
            direction: Direction::ToPeer,
        }
    }

    struct Greedy;
    impl TrafficPolicy for Greedy {
        fn admit(&self, _: &Flow) -> Admit {
            Admit::Reject("test")
        }
        fn grant(&self, _: &Flow, wanted: Unit) -> Grant {
            let Unit::Bytes(n) = wanted else {
                unreachable!()
            };
            Grant {
                bytes: n + 1,
                delay: Duration::ZERO,
            }
        }
        fn observe(&self, _: &Flow, _: Event) {}
    }

    struct Slow;
    impl TrafficPolicy for Slow {
        fn admit(&self, _: &Flow) -> Admit {
            Admit::Accept
        }
        fn grant(&self, _: &Flow, wanted: Unit) -> Grant {
            let Unit::Bytes(n) = wanted else {
                unreachable!()
            };
            Grant {
                bytes: n,
                delay: MAX_GRANT_DELAY + Duration::from_millis(1),
            }
        }
        fn observe(&self, _: &Flow, _: Event) {}
    }

    struct OtherVersion;
    impl TrafficPolicy for OtherVersion {
        fn interface_version(&self) -> u32 {
            POLICY_INTERFACE_VERSION + 1
        }
        fn admit(&self, _: &Flow) -> Admit {
            Admit::Reject("version")
        }
        fn grant(&self, _: &Flow, _: Unit) -> Grant {
            Grant {
                bytes: 0,
                delay: Duration::ZERO,
            }
        }
        fn observe(&self, _: &Flow, _: Event) {}
    }

    #[test]
    fn balanced_passes_everything_through_without_delay() {
        let grant = Balanced.grant(&flow(), Unit::Bytes(16 * 1024));
        assert_eq!(grant.bytes, 16 * 1024);
        assert_eq!(grant.delay, Duration::ZERO);
        assert_eq!(Balanced.admit(&flow()), Admit::Accept);
    }

    #[test]
    fn capabilities_default_to_unknown_and_never_claim_ready() {
        let caps = Capabilities::default();
        assert_eq!(caps.priority_enforcement, Support::Unknown);
        assert!(!caps.all_supported());
        let mut partial = caps;
        partial.write_pacing = Support::Supported;
        assert!(!partial.all_supported());
    }

    #[test]
    fn guard_rejects_over_grant_and_falls_back_to_balanced() {
        let guarded = Guarded::new(Greedy);
        let grant = guarded.grant(&flow(), Unit::Bytes(100));
        assert_eq!(grant.bytes, 100);
        assert_eq!(guarded.violations(), 1);
    }

    #[test]
    fn guard_bounds_the_wait() {
        let guarded = Guarded::new(Slow);
        let grant = guarded.grant(&flow(), Unit::Bytes(100));
        assert_eq!(grant.delay, Duration::ZERO);
        assert_eq!(guarded.violations(), 1);
    }

    #[test]
    fn version_mismatch_keeps_the_pack_off() {
        let guarded = Guarded::new(OtherVersion);
        assert_eq!(guarded.admit(&flow()), Admit::Accept);
        assert_eq!(guarded.grant(&flow(), Unit::Bytes(7)).bytes, 7);
    }

    #[test]
    fn datagram_unit_is_reserved_and_grants_no_bytes() {
        assert_eq!(Balanced.grant(&flow(), Unit::Datagram).bytes, 0);
    }
}
