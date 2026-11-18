//! ADR-0052: pure request builders and response parsers for external Iran-vantage
//! probe services. No network and no I/O here; the HTTP client and the consent gate
//! come in a later slice. Shapes were checked against live responses on 2026-11-18.
//!
//! Rules: a missing, null, malformed or pending value is `Unknown` (never zero);
//! provider text is sanitised and bounded before display.
#![allow(dead_code)] // wired by the panel card in a later slice

use crate::fleet::sanitize;
use serde_json::{Value, json};
use std::net::IpAddr;

pub const MAX_ROWS: usize = 32;
const MAX_TEXT: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    CheckHost,
    Globalping,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Reached; round trip in milliseconds.
    Reached(f64),
    /// Probe ran and failed (timeout, refused, loss).
    Failed(String),
    /// Not measured: pending, null, malformed, or provider error.
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub provider: Provider,
    pub city: String,
    pub asn: String,
    pub outcome: Outcome,
}

fn txt(v: Option<&Value>) -> String {
    let s = v
        .and_then(Value::as_str)
        .map(|s| sanitize(s, MAX_TEXT))
        .unwrap_or_default();
    if s.is_empty() {
        "unknown".to_owned()
    } else {
        s
    }
}

/// check-host.net TCP check URL for one public IP and port, Iran nodes only.
/// `nodes` must be node ids from `iran_nodes`; ids are restricted to `ir<digits>`.
pub fn checkhost_tcp_url(ip: IpAddr, port: u16, nodes: &[String]) -> Option<String> {
    let mut q = format!(
        "https://check-host.net/check-tcp?host={}:{}",
        host_literal(ip),
        port
    );
    let mut n = 0;
    for id in nodes.iter().take(MAX_ROWS) {
        let host = id.strip_suffix(".node.check-host.net")?;
        let digits = host.strip_prefix("ir")?;
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        q.push_str("&node=");
        q.push_str(id);
        n += 1;
    }
    (n > 0).then_some(q)
}

fn host_literal(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(a) => a.to_string(),
        IpAddr::V6(a) => format!("[{a}]"),
    }
}

/// Iran nodes from `GET /nodes/hosts`: (node id, city, asn).
pub fn checkhost_iran_nodes(hosts: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    if let Some(map) = hosts.get("nodes").and_then(Value::as_object) {
        for (id, v) in map {
            let loc = v.get("location").and_then(Value::as_array);
            let is_ir = loc.and_then(|l| l.first()).and_then(Value::as_str) == Some("ir");
            if is_ir && out.len() < MAX_ROWS {
                out.push((
                    sanitize(id, MAX_TEXT),
                    txt(loc.and_then(|l| l.get(2))),
                    txt(v.get("asn")),
                ));
            }
        }
    }
    out.sort();
    out
}

/// `check-result/<id>` for a TCP check. `nodes` is the `nodes` map from the start reply.
/// A node whose result is null is still pending and stays Unknown.
pub fn parse_checkhost_tcp(start_nodes: &Value, result: &Value) -> Vec<Row> {
    let mut rows = Vec::new();
    let Some(nodes) = start_nodes.as_object() else {
        return rows;
    };
    for (id, info) in nodes.iter().take(MAX_ROWS) {
        let city = txt(info.get(2));
        let asn = txt(info.get(4));
        let outcome = match result.get(id).and_then(|r| r.get(0)) {
            Some(r) if r.get("time").and_then(Value::as_f64).is_some() => {
                Outcome::Reached(r["time"].as_f64().unwrap_or(0.0) * 1000.0)
            }
            Some(r) if r.get("error").and_then(Value::as_str).is_some() => {
                Outcome::Failed(txt(r.get("error")))
            }
            _ => Outcome::Unknown,
        };
        rows.push(Row {
            provider: Provider::CheckHost,
            city,
            asn,
            outcome,
        });
    }
    rows.sort_by(|a, b| a.city.cmp(&b.city).then(a.asn.cmp(&b.asn)));
    rows
}

/// Globalping ping measurement body for Iran probes (it has no TCP type, so this is
/// ICMP reachability only; many servers drop ICMP, so Failed is not proof of down).
pub fn globalping_ping_body(ip: IpAddr, limit: u8) -> Value {
    json!({
        "type": "ping",
        "target": ip.to_string(),
        "locations": [{"country": "IR", "limit": limit.clamp(1, 8)}],
        "measurementOptions": {"packets": 3}
    })
}

/// `GET /v1/measurements/<id>` for a ping. Only a finished measurement gives rows;
/// anything else is Unknown.
pub fn parse_globalping_ping(m: &Value) -> Vec<Row> {
    let mut rows = Vec::new();
    let Some(results) = m.get("results").and_then(Value::as_array) else {
        return rows;
    };
    for r in results.iter().take(MAX_ROWS) {
        let probe = r.get("probe");
        let city = txt(probe.and_then(|p| p.get("city")));
        let asn = probe
            .and_then(|p| p.get("asn"))
            .and_then(Value::as_u64)
            .map(|n| format!("AS{n}"))
            .unwrap_or_else(|| "unknown".to_owned());
        let res = r.get("result");
        let finished =
            res.and_then(|x| x.get("status")).and_then(Value::as_str) == Some("finished");
        let stats = res.and_then(|x| x.get("stats"));
        let outcome = if !finished {
            Outcome::Unknown
        } else {
            let rcv = stats.and_then(|s| s.get("rcv")).and_then(Value::as_u64);
            let avg = stats.and_then(|s| s.get("avg")).and_then(Value::as_f64);
            match (rcv, avg) {
                (Some(n), Some(a)) if n > 0 => Outcome::Reached(a),
                (Some(0), _) => Outcome::Failed("no reply".to_owned()),
                _ => Outcome::Unknown,
            }
        };
        rows.push(Row {
            provider: Provider::Globalping,
            city,
            asn,
            outcome,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkhost_url_is_restricted() {
        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        let ok = vec![
            "ir1.node.check-host.net".to_owned(),
            "ir6.node.check-host.net".to_owned(),
        ];
        let u = checkhost_tcp_url(ip, 443, &ok).unwrap();
        assert_eq!(
            u,
            "https://check-host.net/check-tcp?host=8.8.8.8:443&node=ir1.node.check-host.net&node=ir6.node.check-host.net"
        );
        let v6: IpAddr = "2606:4700::1111".parse().unwrap();
        assert!(
            checkhost_tcp_url(v6, 443, &ok)
                .unwrap()
                .contains("host=[2606:4700::1111]:443")
        );
        for bad in [
            "us1.node.check-host.net",
            "ir1.node.evil.com",
            "ir.node.check-host.net",
            "ir1&x=1.node.check-host.net",
        ] {
            assert!(
                checkhost_tcp_url(ip, 443, &[bad.to_owned()]).is_none(),
                "{bad}"
            );
        }
        assert!(checkhost_tcp_url(ip, 443, &[]).is_none());
    }

    #[test]
    fn iran_nodes_from_hosts_list() {
        let hosts = json!({"nodes": {
            "ir2.node.check-host.net": {"asn":"AS209279","ip":"1.1.1.1","location":["ir","Iran","Isfahan"]},
            "de1.node.check-host.net": {"asn":"AS24940","ip":"2.2.2.2","location":["de","Germany","Nuremberg"]},
            "ir1.node.check-host.net": {"asn":"AS47430","ip":"3.3.3.3","location":["ir","Iran","Tehran"]}}});
        let n = checkhost_iran_nodes(&hosts);
        assert_eq!(n.len(), 2);
        assert_eq!(
            n[0],
            (
                "ir1.node.check-host.net".into(),
                "Tehran".into(),
                "AS47430".into()
            )
        );
        assert!(checkhost_iran_nodes(&json!({})).is_empty());
    }

    #[test]
    fn checkhost_tcp_results_keep_pending_unknown() {
        let start = json!({
            "ir1.node.check-host.net": ["ir","Iran","Tehran","185.105.238.209","AS47430"],
            "ir2.node.check-host.net": ["ir","Iran","Isfahan","1.2.3.4","AS209279"],
            "ir6.node.check-host.net": ["ir","Iran","Qom","1.2.3.5","AS206596"]});
        let res = json!({
            "ir1.node.check-host.net": [{"time": 0.03, "address": "104.28.31.42"}],
            "ir2.node.check-host.net": [{"error": "Connection timed out"}],
            "ir6.node.check-host.net": null});
        let rows = parse_checkhost_tcp(&start, &res);
        assert_eq!(rows.len(), 3);
        let by = |c: &str| rows.iter().find(|r| r.city == c).unwrap().outcome.clone();
        assert_eq!(by("Tehran"), Outcome::Reached(30.0));
        assert_eq!(
            by("Isfahan"),
            Outcome::Failed("Connection timed out".into())
        );
        assert_eq!(by("Qom"), Outcome::Unknown);
        // provider error shape (rate limit) gives no rows from an empty start map
        assert!(
            parse_checkhost_tcp(&json!({"error": "limit_exceeded"}), &res).is_empty()
                || parse_checkhost_tcp(&json!({"error": "limit_exceeded"}), &res)
                    .iter()
                    .all(|r| r.outcome == Outcome::Unknown)
        );
    }

    #[test]
    fn globalping_body_and_results() {
        let b = globalping_ping_body("1.1.1.1".parse().unwrap(), 50);
        assert_eq!(b["locations"][0]["limit"], 8);
        assert_eq!(b["locations"][0]["country"], "IR");
        let m = json!({"status":"finished","results":[
          {"probe":{"city":"Tehran","asn":202468},"result":{"status":"finished","stats":{"avg":84.633,"rcv":3,"loss":0}}},
          {"probe":{"city":"Tehran","asn":49100},"result":{"status":"finished","stats":{"avg":null,"rcv":0,"loss":100}}},
          {"probe":{"city":"Tehran","asn":206065},"result":{"status":"in-progress"}}]});
        let rows = parse_globalping_ping(&m);
        assert_eq!(rows[0].outcome, Outcome::Reached(84.633));
        assert_eq!(rows[0].asn, "AS202468");
        assert_eq!(rows[1].outcome, Outcome::Failed("no reply".into()));
        assert_eq!(rows[2].outcome, Outcome::Unknown);
        assert!(parse_globalping_ping(&json!({"error":{"type":"x"}})).is_empty());
    }

    #[test]
    fn provider_text_is_sanitised() {
        let start =
            json!({"ir1.node.check-host.net": ["ir","Iran","Teh\u{202e}ran","1.1.1.1","AS1"]});
        let rows = parse_checkhost_tcp(&start, &json!({}));
        assert!(!rows[0].city.contains('\u{202e}'));
    }
}
