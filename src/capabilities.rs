//! Engine capability contract (versioned, read-only).
//!
//! The catalogue is built from facts this build really has: which engines are compiled in, which
//! configuration they accept and what has been verified. Anything not established is the string
//! "unknown", never "supported". The BAFT engine is intentionally absent until its acceptance
//! gates pass (ADR-0019); it must not be listed here or offered by the panel before that.

use serde_json::{Value, json};

/// Bump when a field is removed or changes meaning. Adding fields keeps the version.
pub const CONTRACT_VERSION: u32 = 1;

fn tcp_tls() -> Value {
    json!({
        "id": "tcp-tls",
        "status": "stable",
        "directions": ["direct", "reverse"],
        "security": "TLS 1.3 mutual authentication, offline CA, mandatory CRL",
        "transport": {"carrier": "tcp", "encapsulation": "tls1.3-mtls+yamux", "udp": false},
        "roles": {"listener": true, "dialer": true},
        "network": {"ipv4": "supported", "ipv6": "unknown", "dns": "optional"},
        "identity": {
            "kind": "x509",
            "requires_ca": true,
            "requires_crl": true,
            "sni": "required",
            "san": "uri-verified",
            "peer_pinning": "sha256-fingerprint",
            "certificate_validation": "chain-to-offline-ca-and-crl"
        },
        "platform": {"os": ["linux"], "min_agent": "0.1.0"},
        "evidence": {
            "unit_tests": true,
            "lab_two_host": {"direct": "pass", "reverse": "pass"},
            "source": "docs/PHASE1_IMPLEMENTATION_STATUS.md"
        }
    })
}

fn noise_tcp() -> Value {
    json!({
        "id": "noise-tcp",
        "status": "experimental",
        "directions": ["direct", "reverse"],
        "security": "Noise static keys pinned per peer, no CA",
        "transport": {"carrier": "tcp", "encapsulation": "noise-ik+yamux", "udp": false},
        "roles": {"listener": true, "dialer": true},
        "network": {"ipv4": "supported", "ipv6": "unknown", "dns": "optional"},
        "identity": {
            "kind": "noise-static-key",
            "requires_ca": false,
            "requires_crl": false,
            "sni": "not-applicable",
            "san": "not-applicable",
            "peer_pinning": "static-public-key",
            "certificate_validation": "not-applicable"
        },
        "platform": {"os": ["linux"], "min_agent": "0.1.0"},
        "evidence": {
            "unit_tests": true,
            "lab_two_host": {"direct": "pass", "reverse": "unknown"},
            "source": "docs/PHASE1_IMPLEMENTATION_STATUS.md"
        }
    })
}

/// What Direct and Reverse mean in this build (ADR-0011). The two peers must configure the same
/// label; it is checked in the authorization hello. The data path is identical for both: the
/// `serve` node listens on the tunnel port and forwards to its local service, the `forward` node
/// dials that port and exposes the service on its own loopback. The label does not change who
/// dials, so no "reverse connection initiator" is claimed.
fn direction_model() -> Value {
    json!({
        "label_agreed_by_both_peers": true,
        "serve": {"listens_on_tunnel_port": true, "dials_peer": false, "forwards_to": "local service"},
        "forward": {"listens_on_tunnel_port": false, "dials_peer": true, "exposes": "service on local loopback"},
        "direct": {"serve_role": "listener", "forward_role": "dialer"},
        "reverse": {"serve_role": "listener", "forward_role": "dialer", "note": "same data path; the label is an authorization decision"},
        "nat": "the dialing side needs outbound reachability to the listener's tunnel port; the listener needs that port reachable inbound",
        "firewall": "inbound allow on the listener's tunnel port; not inspected by the agent (unknown)"
    })
}

/// Limits and timers of the shared tunnel layer, read from the constants the runtime really uses.
fn runtime_limits() -> Value {
    use crate::tunnel as t;
    json!({
        "handshake_timeout_s": t::HANDSHAKE_TIMEOUT.as_secs(),
        "connect_timeout_s": t::CONNECT_TIMEOUT.as_secs(),
        "keepalive_interval_s": t::KEEPALIVE_INTERVAL.as_secs(),
        "stream_idle_timeout_s": t::STREAM_IDLE_TIMEOUT.as_secs(),
        "reconnect": {"automatic": true, "initial_backoff_s": t::INITIAL_BACKOFF.as_secs(), "max_backoff_s": t::MAX_BACKOFF.as_secs()},
        "max_sessions": t::MAX_SESSIONS,
        "default_max_streams": crate::config::default_max_streams()
    })
}

/// The `/v1/engines` document. Legacy keys (`id`, `status`, `directions`, `security`) are kept.
pub fn catalog() -> Value {
    json!({
        "contract_version": CONTRACT_VERSION,
        "agent_version": env!("CARGO_PKG_VERSION"),
        "platform": {"os": std::env::consts::OS, "arch": std::env::consts::ARCH},
        "direction_model": direction_model(),
        "runtime": runtime_limits(),
        "engines": [tcp_tls(), noise_tcp()]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_is_versioned_and_keeps_legacy_fields() {
        let doc = catalog();
        assert_eq!(doc["contract_version"], 1);
        let engines = doc["engines"].as_array().unwrap();
        assert_eq!(engines.len(), 2);
        for e in engines {
            for key in [
                "id",
                "status",
                "directions",
                "security",
                "identity",
                "evidence",
            ] {
                assert!(e.get(key).is_some(), "missing {key}");
            }
        }
    }

    #[test]
    fn runtime_limits_come_from_the_runtime_and_reverse_makes_no_initiator_claim() {
        let doc = catalog();
        assert_eq!(doc["runtime"]["keepalive_interval_s"], 15);
        assert_eq!(doc["runtime"]["reconnect"]["max_backoff_s"], 60);
        assert_eq!(doc["direction_model"]["reverse"]["forward_role"], "dialer");
        assert!(
            doc["direction_model"]["reverse"]["note"]
                .as_str()
                .unwrap()
                .contains("authorization")
        );
    }

    #[test]
    fn unverified_capabilities_are_unknown_and_baft_is_not_listed() {
        let doc = catalog();
        let engines = doc["engines"].as_array().unwrap();
        assert!(engines.iter().all(|e| e["id"] != "baft"));
        for e in engines {
            assert_eq!(e["network"]["ipv6"], "unknown");
        }
        let noise = engines.iter().find(|e| e["id"] == "noise-tcp").unwrap();
        assert_eq!(noise["evidence"]["lab_two_host"]["reverse"], "unknown");
    }
}
