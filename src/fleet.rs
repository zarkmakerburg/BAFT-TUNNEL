//! Fleet F1 core (ADR-0045): pure, I/O-free rules for the read-only hub. Nothing here opens a
//! socket or reads a file; the hub listener is a separate, still gated slice. The module only
//! fixes the security-relevant decisions so they can be tested before any transport exists:
//! certificate profile/EKU (F-2, F-4), identity taken from the certificate (F-5), strict report
//! bounds and text sanitising (F-9, F-13), `seq` replay rules (F-6), freshness (Live/Stale/Never)
//! and the fail-closed CRL rule (F-8).
#![allow(dead_code)]

use serde_json::Value;
use std::collections::BTreeMap;

/// Provisional EKU OIDs, one per profile (F-4). Placeholders until the profile document assigns
/// the final arc; the *rule* (exactly one BAFT EKU, matching the profile) is what is tested.
pub const EKU_TUNNEL: &str = "1.3.6.1.4.1.59999.1.1";
pub const EKU_FLEET_NODE: &str = "1.3.6.1.4.1.59999.1.2";
pub const EKU_FLEET_HUB: &str = "1.3.6.1.4.1.59999.1.3";
pub const EKU_FLEET_HUB_SIGNER: &str = "1.3.6.1.4.1.59999.1.4";

/// Bounds (F-9). The numbers are part of the protocol and tested.
pub const MAX_REPORT_BYTES: usize = 16 * 1024;
pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_EVENTS: usize = 32;
pub const MAX_EVENT_CHARS: usize = 200;
pub const MAX_INSTANCES: usize = 16;
pub const MAX_NODES: usize = 256;
/// Minimum spacing between accepted reports from one node.
pub const MIN_REPORT_INTERVAL_S: u64 = 5;
/// Reports are expected this often; Live < 3 intervals.
pub const REPORT_INTERVAL_S: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Tunnel,
    FleetNode,
    FleetHub,
    FleetHubSigner,
}

impl Profile {
    fn eku(self) -> &'static str {
        match self {
            Profile::Tunnel => EKU_TUNNEL,
            Profile::FleetNode => EKU_FLEET_NODE,
            Profile::FleetHub => EKU_FLEET_HUB,
            Profile::FleetHubSigner => EKU_FLEET_HUB_SIGNER,
        }
    }
}

fn is_baft_eku(oid: &str) -> bool {
    [
        EKU_TUNNEL,
        EKU_FLEET_NODE,
        EKU_FLEET_HUB,
        EKU_FLEET_HUB_SIGNER,
    ]
    .contains(&oid)
}

/// A certificate may be used for `want` only if it carries exactly that profile's EKU and no other
/// BAFT EKU (F-2: a tunnel or node certificate is never a hub; F-4: no multi-purpose certificates).
pub fn profile_allows(ekus: &[&str], want: Profile) -> bool {
    let baft: Vec<&&str> = ekus.iter().filter(|e| is_baft_eku(e)).collect();
    baft.len() == 1 && *baft[0] == want.eku()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    Profile,
    TooLarge,
    Malformed(&'static str),
    UnknownVersion,
    Replay,
    TooFast,
    InventoryFull,
}

/// Strip control and bidirectional-override characters and cap the length (F-13).
pub fn sanitize(text: &str, max_chars: usize) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(*c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .take(max_chars)
        .collect()
}

/// What the hub keeps per node. Identity fields come from the certificate, never the body (F-5).
#[derive(Debug, Clone, PartialEq)]
pub struct NodeRecord {
    pub node_id: String,
    pub cert_serial: String,
    pub last_seq: u64,
    pub last_report_unix: u64,
    pub version: Option<String>,
    pub instances: Vec<Instance>,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub name: String,
    /// Reported tunnel state as text; kept separate from `integrity` and `health` (never merged).
    pub tunnel: Option<String>,
    pub integrity: Option<String>,
    pub health: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    Live,
    Stale { age_s: u64 },
    Never,
}

/// Verified identity of the TLS client, produced by the (future) transport after chain, CRL and
/// profile checks; this module re-checks the profile so it cannot be skipped by mistake.
pub struct PeerCert<'a> {
    pub node_id: &'a str,
    pub serial: &'a str,
    pub ekus: &'a [&'a str],
}

#[derive(Default)]
pub struct Hub {
    nodes: BTreeMap<String, NodeRecord>,
}

fn text(v: &Value, key: &str, max: usize) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(|s| sanitize(s, max))
}

impl Hub {
    pub fn nodes(&self) -> impl Iterator<Item = &NodeRecord> {
        self.nodes.values()
    }

    /// Accept one report (`node.status.v1`). `now` is unix seconds.
    pub fn accept(&mut self, peer: &PeerCert, body: &[u8], now: u64) -> Result<(), Reject> {
        if !profile_allows(peer.ekus, Profile::FleetNode) {
            return Err(Reject::Profile);
        }
        if body.len() > MAX_REPORT_BYTES {
            return Err(Reject::TooLarge);
        }
        let v: Value = serde_json::from_slice(body).map_err(|_| Reject::Malformed("json"))?;
        if v.get("schema").and_then(Value::as_str) != Some("node.status.v1") {
            return Err(Reject::UnknownVersion);
        }
        let seq = v
            .get("seq")
            .and_then(Value::as_u64)
            .ok_or(Reject::Malformed("seq"))?;
        let prior = self.nodes.get(peer.node_id);
        if let Some(p) = prior {
            // A restarted node reuses nothing: seq may only restart under a new certificate (F-6).
            if p.cert_serial == peer.serial {
                if seq <= p.last_seq {
                    return Err(Reject::Replay);
                }
                if now.saturating_sub(p.last_report_unix) < MIN_REPORT_INTERVAL_S {
                    return Err(Reject::TooFast);
                }
            }
        } else if self.nodes.len() >= MAX_NODES {
            return Err(Reject::InventoryFull);
        }
        let instances = match v.get("instances") {
            None => vec![],
            Some(Value::Array(a)) if a.len() <= MAX_INSTANCES => a
                .iter()
                .map(|i| {
                    Ok(Instance {
                        name: text(i, "name", MAX_NAME_CHARS)
                            .ok_or(Reject::Malformed("instance"))?,
                        tunnel: text(i, "tunnel", MAX_NAME_CHARS),
                        integrity: text(i, "integrity", MAX_NAME_CHARS),
                        health: text(i, "health", MAX_NAME_CHARS),
                    })
                })
                .collect::<Result<Vec<_>, Reject>>()?,
            Some(_) => return Err(Reject::Malformed("instances")),
        };
        let events = match v.get("events") {
            None => vec![],
            Some(Value::Array(a)) if a.len() <= MAX_EVENTS => a
                .iter()
                .filter_map(Value::as_str)
                .map(|s| sanitize(s, MAX_EVENT_CHARS))
                .collect(),
            Some(_) => return Err(Reject::Malformed("events")),
        };
        self.nodes.insert(
            peer.node_id.to_owned(),
            NodeRecord {
                node_id: peer.node_id.to_owned(),
                cert_serial: peer.serial.to_owned(),
                last_seq: seq,
                last_report_unix: now,
                version: text(&v, "version", MAX_NAME_CHARS),
                instances,
                events,
            },
        );
        Ok(())
    }

    /// Freshness is derived on read; a stale or absent node's fields must be shown as Unknown.
    pub fn freshness(&self, node_id: &str, now: u64) -> Freshness {
        match self.nodes.get(node_id) {
            None => Freshness::Never,
            Some(n) => {
                let age = now.saturating_sub(n.last_report_unix);
                if age < 3 * REPORT_INTERVAL_S {
                    Freshness::Live
                } else {
                    Freshness::Stale { age_s: age }
                }
            }
        }
    }
}

/// A job may only be acted on while the CRL is current (F-8); reports merely warn.
pub fn crl_allows_jobs(crl_next_update_unix: Option<u64>, now: u64) -> bool {
    matches!(crl_next_update_unix, Some(n) if n > now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer<'a>(id: &'a str, serial: &'a str, ekus: &'a [&'a str]) -> PeerCert<'a> {
        PeerCert {
            node_id: id,
            serial,
            ekus,
        }
    }
    const NODE: &[&str] = &[EKU_FLEET_NODE];
    fn body(seq: u64) -> Vec<u8> {
        format!(r#"{{"schema":"node.status.v1","seq":{seq},"version":"0.3.0","instances":[{{"name":"a","tunnel":"up"}}]}}"#).into_bytes()
    }

    #[test]
    fn profile_rules() {
        assert!(profile_allows(&[EKU_FLEET_NODE], Profile::FleetNode));
        assert!(!profile_allows(&[EKU_TUNNEL], Profile::FleetNode)); // tunnel cert is not a node
        assert!(!profile_allows(&[EKU_FLEET_NODE], Profile::FleetHub)); // node cert is not a hub (F-2)
        assert!(!profile_allows(
            &[EKU_FLEET_NODE, EKU_TUNNEL],
            Profile::FleetNode
        )); // multi-EKU (F-4)
        assert!(profile_allows(
            &[EKU_FLEET_NODE, "1.3.6.1.5.5.7.3.2"],
            Profile::FleetNode
        )); // clientAuth ok
        assert!(!profile_allows(&[], Profile::FleetNode));
    }

    #[test]
    fn rejects_wrong_profile_oversize_and_bad_schema() {
        let mut h = Hub::default();
        assert_eq!(
            h.accept(&peer("n", "1", &[EKU_TUNNEL]), &body(1), 100),
            Err(Reject::Profile)
        );
        let big = vec![b' '; MAX_REPORT_BYTES + 1];
        assert_eq!(
            h.accept(&peer("n", "1", NODE), &big, 100),
            Err(Reject::TooLarge)
        );
        assert_eq!(
            h.accept(&peer("n", "1", NODE), b"{", 100),
            Err(Reject::Malformed("json"))
        );
        assert_eq!(
            h.accept(
                &peer("n", "1", NODE),
                br#"{"schema":"node.status.v9","seq":1}"#,
                100
            ),
            Err(Reject::UnknownVersion)
        );
        assert_eq!(h.nodes().count(), 0);
    }

    #[test]
    fn identity_comes_from_certificate_not_body() {
        let mut h = Hub::default();
        let b = br#"{"schema":"node.status.v1","seq":1,"node_id":"evil","instances":[]}"#;
        h.accept(&peer("real", "1", NODE), b, 100).unwrap();
        let ids: Vec<_> = h.nodes().map(|n| n.node_id.as_str()).collect();
        assert_eq!(ids, ["real"]);
    }

    #[test]
    fn seq_replay_and_restart_rules() {
        let mut h = Hub::default();
        h.accept(&peer("n", "1", NODE), &body(5), 100).unwrap();
        assert_eq!(
            h.accept(&peer("n", "1", NODE), &body(5), 200),
            Err(Reject::Replay)
        );
        assert_eq!(
            h.accept(&peer("n", "1", NODE), &body(4), 200),
            Err(Reject::Replay)
        );
        assert_eq!(
            h.accept(&peer("n", "1", NODE), &body(6), 101),
            Err(Reject::TooFast)
        );
        h.accept(&peer("n", "1", NODE), &body(6), 110).unwrap();
        // A reinstall restarts seq only under a new certificate serial.
        h.accept(&peer("n", "2", NODE), &body(1), 111).unwrap();
    }

    #[test]
    fn bounds_and_sanitising() {
        let mut h = Hub::default();
        let many: Vec<String> = (0..MAX_INSTANCES + 1)
            .map(|i| format!(r#"{{"name":"i{i}"}}"#))
            .collect();
        let b = format!(
            r#"{{"schema":"node.status.v1","seq":1,"instances":[{}]}}"#,
            many.join(",")
        );
        assert_eq!(
            h.accept(&peer("n", "1", NODE), b.as_bytes(), 100),
            Err(Reject::Malformed("instances"))
        );
        let evil = "ok\u{202E}gnp\u{2066}x\n\u{0007}";
        assert_eq!(sanitize(evil, 50), "okgnpx");
        assert_eq!(
            sanitize(&"a".repeat(500), MAX_EVENT_CHARS).chars().count(),
            MAX_EVENT_CHARS
        );
        let ev = format!(
            r#"{{"schema":"node.status.v1","seq":1,"events":["a{}b"]}}"#,
            '\u{202e}'
        );
        h.accept(&peer("n", "1", NODE), ev.as_bytes(), 100).unwrap();
        assert_eq!(h.nodes().next().unwrap().events, ["ab"]);
    }

    #[test]
    fn inventory_cap() {
        let mut h = Hub::default();
        for i in 0..MAX_NODES {
            let id = format!("n{i}");
            h.accept(&peer(&id, "1", NODE), &body(1), 100).unwrap();
        }
        assert_eq!(
            h.accept(&peer("extra", "1", NODE), &body(1), 100),
            Err(Reject::InventoryFull)
        );
    }

    #[test]
    fn freshness_is_derived_and_never_for_unknown() {
        let mut h = Hub::default();
        assert_eq!(h.freshness("n", 100), Freshness::Never);
        h.accept(&peer("n", "1", NODE), &body(1), 100).unwrap();
        assert_eq!(
            h.freshness("n", 100 + 3 * REPORT_INTERVAL_S - 1),
            Freshness::Live
        );
        assert_eq!(
            h.freshness("n", 100 + 3 * REPORT_INTERVAL_S),
            Freshness::Stale { age_s: 90 }
        );
    }

    #[test]
    fn crl_fails_closed_for_jobs() {
        assert!(crl_allows_jobs(Some(200), 100));
        assert!(!crl_allows_jobs(Some(100), 100));
        assert!(!crl_allows_jobs(None, 100));
    }
}
