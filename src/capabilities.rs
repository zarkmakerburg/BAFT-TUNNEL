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

/// The `/v1/engines` document. Legacy keys (`id`, `status`, `directions`, `security`) are kept.
pub fn catalog() -> Value {
    json!({
        "contract_version": CONTRACT_VERSION,
        "agent_version": env!("CARGO_PKG_VERSION"),
        "platform": {"os": std::env::consts::OS, "arch": std::env::consts::ARCH},
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
