//! ADR-0052: panel API for the server health check (`/v1/probe/consent`, `/v1/probe/run`).
//!
//! The operator switches each external provider on or off (default off, stored in the
//! state directory). A run measures from this server first, then asks only the enabled
//! providers. Results are cached for 10 minutes per target; every value that was not
//! measured is reported as `unknown` with a reason.

use crate::probe_adapters::{Outcome, Row};
use crate::probe_http::{self, Consent, Report};
use crate::target_guard::{self, LookupGate};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CACHE_TTL: Duration = Duration::from_secs(600);
const CONSENT_FILE: &str = "probe_consent.json";

struct Shared {
    gate: LookupGate,
    cache: HashMap<(IpAddr, u16), (Instant, Value)>,
}

fn shared() -> &'static Mutex<Shared> {
    static S: OnceLock<Mutex<Shared>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(Shared {
            gate: LookupGate::new(600, 3, 10),
            cache: HashMap::new(),
        })
    })
}

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn read_consent(state_dir: Option<&Path>) -> Consent {
    let Some(dir) = state_dir else {
        return Consent::default();
    };
    let Ok(raw) = std::fs::read(dir.join(CONSENT_FILE)) else {
        return Consent::default();
    };
    let v: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    Consent {
        checkhost: v.get("checkhost").and_then(Value::as_bool).unwrap_or(false),
        globalping: v
            .get("globalping")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn write_consent(dir: &Path, c: Consent) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = dir.join(format!("{CONSENT_FILE}.tmp"));
    let body = json!({"checkhost": c.checkhost, "globalping": c.globalping}).to_string();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(body.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(tmp, dir.join(CONSENT_FILE))
}

pub fn consent_json(c: Consent) -> Value {
    json!({"checkhost": c.checkhost, "globalping": c.globalping,
           "providers": {"checkhost": "check-host.net", "globalping": "api.globalping.io"}})
}

/// `POST /v1/probe/consent` `{"provider":"checkhost"|"globalping","enabled":bool,"confirm":true}`.
/// Enabling needs `confirm: true`, which the panel sets only after showing the privacy text.
pub fn consent_set(state_dir: Option<&Path>, body: &[u8]) -> (u16, Value) {
    let Some(dir) = state_dir else {
        return (409, json!({"error": "no state directory"}));
    };
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return (422, json!({"error": "invalid request"}));
    };
    let (Some(p), Some(on)) = (
        v.get("provider").and_then(Value::as_str),
        v.get("enabled").and_then(Value::as_bool),
    ) else {
        return (422, json!({"error": "invalid request"}));
    };
    if on && v.get("confirm").and_then(Value::as_bool) != Some(true) {
        return (422, json!({"error": "enabling needs \"confirm\": true"}));
    }
    let mut c = read_consent(Some(dir));
    match p {
        "checkhost" => c.checkhost = on,
        "globalping" => c.globalping = on,
        _ => return (422, json!({"error": "unknown provider"})),
    }
    match write_consent(dir, c) {
        Ok(()) => (200, consent_json(c)),
        Err(_) => (500, json!({"error": "could not save"})),
    }
}

fn row_json(r: &Row) -> Value {
    let (state, extra) = match &r.outcome {
        Outcome::Reached(ms) => ("reached", json!({"ms": (ms * 10.0).round() / 10.0})),
        Outcome::Failed(why) => ("failed", json!({"reason": why})),
        Outcome::Unknown => ("unknown", json!({})),
    };
    let mut o = json!({"city": r.city, "asn": r.asn, "state": state});
    if let (Some(m), Some(e)) = (o.as_object_mut(), extra.as_object()) {
        m.extend(e.clone());
    }
    o
}

fn provider_json(name: &str, enabled: bool, rep: Option<&Report>, note: &str) -> Value {
    json!({"provider": name, "enabled": enabled,
           "note": rep.map(|r| r.note).unwrap_or(note),
           "rows": rep.map(|r| r.rows.iter().map(row_json).collect::<Vec<_>>()).unwrap_or_default()})
}

async fn from_here(ip: IpAddr, port: u16) -> Value {
    let t = Instant::now();
    match tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::TcpStream::connect(SocketAddr::new(ip, port)),
    )
    .await
    {
        Ok(Ok(_)) => {
            json!({"state": "reached", "ms": t.elapsed().as_millis() as u64, "label": "measured from this server"})
        }
        Ok(Err(_)) => json!({"state": "failed", "label": "measured from this server"}),
        Err(_) => {
            json!({"state": "failed", "reason": "timeout", "label": "measured from this server"})
        }
    }
}

/// `POST /v1/probe/run` `{"ip":"...","port":443}`.
pub async fn run(state_dir: Option<&Path>, body: &[u8]) -> (u16, Value) {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return (422, json!({"error": "invalid request"}));
    };
    let ip = v
        .get("ip")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<IpAddr>().ok());
    let port = v
        .get("port")
        .and_then(Value::as_u64)
        .filter(|p| (1..=65535).contains(p))
        .map(|p| p as u16);
    let (Some(ip), Some(port)) = (ip, port) else {
        return (
            422,
            json!({"error": "an IP address and a port are required"}),
        );
    };
    // Uniform refusal: private, loopback, link-local, metadata and reserved targets.
    let Ok(ip) = target_guard::check_public(ip) else {
        return (422, json!({"error": "target must be a public IP address"}));
    };
    if let Ok(s) = shared().lock() {
        if let Some((at, cached)) = s.cache.get(&(ip, port)) {
            if at.elapsed() < CACHE_TTL {
                let mut out = cached.clone();
                out["cached_s"] = json!(at.elapsed().as_secs());
                return (200, out);
            }
        }
    }
    let consent = read_consent(state_dir);
    let here = from_here(ip, port).await;
    let ch = if consent.checkhost {
        // The budget gate lives in the shared state; take it out for the await.
        let mut gate = match shared().lock() {
            Ok(mut s) => std::mem::replace(&mut s.gate, LookupGate::new(600, 3, 10)),
            Err(_) => LookupGate::new(600, 3, 10),
        };
        let rep = probe_http::run_checkhost(ip, port, consent, &mut gate, now_s()).await;
        if let Ok(mut s) = shared().lock() {
            s.gate = gate;
        }
        Some(rep)
    } else {
        None
    };
    let out = json!({
        "target": ip.to_string(), "port": port,
        "country": {"value": null, "source": "none", "reason": "no offline GeoIP database installed"},
        "suggested_role": target_guard::suggest_role(None),
        "from_here": here,
        "providers": [
            provider_json("check-host", consent.checkhost, ch.as_ref(), "disabled"),
            provider_json("globalping", consent.globalping, None, if consent.globalping { "not available yet" } else { "disabled" }),
        ],
        "cached_s": 0,
    });
    // Only cache runs that actually measured something outside this server.
    let measured = ch.as_ref().is_some_and(|r| !r.rows.is_empty());
    if let Ok(mut s) = shared().lock() {
        if measured {
            s.cache.insert((ip, port), (Instant::now(), out.clone()));
        }
        if s.cache.len() > 64 {
            s.cache.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
        }
    }
    (200, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("baft-probe-api-{}-{}", std::process::id(), now_s()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn consent_defaults_off_and_needs_confirm() {
        let d = dir();
        assert_eq!(read_consent(Some(&d)), Consent::default());
        assert_eq!(read_consent(None), Consent::default());
        let (c, _) = consent_set(Some(&d), br#"{"provider":"checkhost","enabled":true}"#);
        assert_eq!(c, 422);
        assert_eq!(read_consent(Some(&d)), Consent::default());
        let (c, body) = consent_set(
            Some(&d),
            br#"{"provider":"checkhost","enabled":true,"confirm":true}"#,
        );
        assert_eq!(c, 200);
        assert_eq!(body["checkhost"], true);
        assert_eq!(body["globalping"], false);
        assert!(read_consent(Some(&d)).checkhost);
        let (c, _) = consent_set(Some(&d), br#"{"provider":"checkhost","enabled":false}"#);
        assert_eq!(c, 200);
        assert!(!read_consent(Some(&d)).checkhost);
        assert_eq!(
            consent_set(Some(&d), br#"{"provider":"x","enabled":false}"#).0,
            422
        );
        assert_eq!(consent_set(None, b"{}").0, 409);
        let _ = std::fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn run_refuses_non_public_and_bad_input() {
        for b in [
            &br#"{"ip":"127.0.0.1","port":80}"#[..],
            br#"{"ip":"10.1.2.3","port":80}"#,
            br#"{"ip":"169.254.169.254","port":80}"#,
            br#"{"ip":"example.com","port":80}"#,
            br#"{"ip":"8.8.8.8","port":0}"#,
            br#"{"ip":"8.8.8.8"}"#,
            b"nope",
        ] {
            assert_eq!(run(None, b).await.0, 422, "{}", String::from_utf8_lossy(b));
        }
    }

    #[tokio::test]
    async fn without_consent_no_provider_is_contacted_and_geo_is_unknown() {
        let (c, v) = run(None, br#"{"ip":"192.0.0.9","port":9}"#).await;
        // 192.0.0.9 is public by our rules; it only exercises the local check.
        assert_eq!(c, 200);
        assert_eq!(v["providers"][0]["enabled"], false);
        assert_eq!(v["providers"][0]["note"], "disabled");
        assert!(v["providers"][0]["rows"].as_array().unwrap().is_empty());
        assert!(v["country"]["value"].is_null());
        assert!(v["suggested_role"].is_null());
        assert_eq!(v["from_here"]["label"], "measured from this server");
    }
}
