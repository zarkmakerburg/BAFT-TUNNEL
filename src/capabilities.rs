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
        "reverse": {
            "state": "preview",
            "reason": "real Reverse (ADR-0021): the service node dials out, the public node owns the user ingress; proven once on a two-host LAB, not yet production-grade",
            "evidence": {"lab_run": "42506805986", "sha": "2d8cd5e", "checks": "outbound-only, 64KiB+2MiB integrity, reconnect both sides, busy port, wrong identity, wrong pin, revoked certificate"},
            "open": "R3 breadth: agent restart, repeated runs, longer soak"
        },
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
            "reverse_lab_run": "42506805986",
            "source": "docs/PHASE1_IMPLEMENTATION_STATUS.md"
        }
    })
}

fn noise_tcp() -> Value {
    json!({
        "id": "noise-tcp",
        "status": "experimental",
        "directions": ["direct"],
        "reverse": {
            "state": "unproven",
            "reason": "implemented in the core and config, but no two-host LAB proof for Noise yet (ADR-0021 R3); not offered and blocked by preflight"
        },
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
            "lab_two_host": {"direct": "pass", "reverse": "unproven"},
            "source": "docs/PHASE1_IMPLEMENTATION_STATUS.md"
        }
    })
}

/// The verified connection model of this build (ADR-0021). Direct: the `serve` node binds the
/// tunnel port, the `forward` node dials it, owns the loopback ingress and opens one stream per
/// connection, and `serve` dials the fixed target. Reverse (tcp-tls only, preview): the same
/// listener/dialer roles, but the service node (the dialer) dials out and dials the fixed target,
/// while the public node (the listener) owns the user ingress and opens the streams. Reverse for
/// Noise is implemented but unproven and is not offered. BAFT is not part of this model.
fn direction_model() -> Value {
    json!({
        "topology_contract_version": 2,
        "label_agreed_by_both_peers": true,
        "implemented": {
            "direct": {
                "connection_initiator": "forward",
                "connection_listener": "serve",
                "traffic_ingress": "forward (local loopback listener)",
                "traffic_egress": "serve (fixed target)",
                "service_target": "next to the serve node",
                "return_path": "same stream, same connection",
                "relay_node": "none",
                "public_reachability_needed": "serve (inbound tunnel port)"
            },
            "reverse": {
                "engines": ["tcp-tls"],
                "state": "preview",
                "connection_initiator": "forward (the service node, outbound only)",
                "connection_listener": "serve (the public node)",
                "traffic_ingress": "serve (user ingress listener, loopback unless explicitly allowed)",
                "traffic_egress": "forward (fixed target next to the service)",
                "service_target": "next to the forward (service) node",
                "return_path": "same stream, same connection",
                "relay_node": "none",
                "public_reachability_needed": "serve (inbound tunnel port); the service node needs no inbound port"
            }
        },
        "reverse": {
            "state": "preview",
            "engines": {"tcp-tls": "preview (two-host LAB run 42506805986)", "noise-tcp": "unproven (no two-host LAB)", "baft": "not part of Rust Reverse; hidden"},
            "open": "R3 breadth (agent restart, repeated runs, soak) before it can be called stable"
        },
        "nat": "the dialing side needs outbound reachability to the listener's tunnel port; the listener needs that port reachable inbound",
        "firewall": "inbound allow on the listener's tunnel port; not inspected by the agent (unknown)"
    })
}

/// Whether `direction` is operational for `engine` in this build.
pub fn supports_direction(engine: &str, direction: &str) -> bool {
    catalog()["engines"]
        .as_array()
        .and_then(|list| list.iter().find(|e| e["id"] == engine))
        .and_then(|e| e["directions"].as_array())
        .is_some_and(|d| d.iter().any(|x| x == direction))
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
    fn runtime_limits_come_from_the_runtime_and_reverse_is_preview_for_tcp_tls_only() {
        let doc = catalog();
        assert_eq!(doc["runtime"]["keepalive_interval_s"], 15);
        assert_eq!(doc["runtime"]["reconnect"]["max_backoff_s"], 60);
        assert_eq!(doc["direction_model"]["reverse"]["state"], "preview");
        assert_eq!(
            doc["direction_model"]["implemented"]["direct"]["connection_initiator"],
            "forward"
        );
        assert!(supports_direction("tcp-tls", "direct"));
        assert!(supports_direction("tcp-tls", "reverse"));
        assert!(!supports_direction("noise-tcp", "reverse"));
        assert!(!supports_direction("baft", "reverse"));
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
        assert_eq!(noise["evidence"]["lab_two_host"]["reverse"], "unproven");
        assert_eq!(noise["reverse"]["state"], "unproven");
        let tls = engines.iter().find(|e| e["id"] == "tcp-tls").unwrap();
        assert_eq!(tls["reverse"]["state"], "preview");
        assert_eq!(tls["evidence"]["reverse_lab_run"], "42506805986");
    }
}
