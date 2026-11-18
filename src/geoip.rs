//! ADR-0052: offline country lookup from a local DB-IP "IP to Country Lite" CSV
//! (`start,end,CC` per line, IPv4 and IPv6). No network and no IP leaves the host.
//! The file is placed by `scripts/geoip-update.sh` (operator run, CC BY 4.0, attribution
//! below). A missing, empty or malformed file means Unknown, never a guess.
#![allow(dead_code)] // used by the probe API in some builds only

use std::net::IpAddr;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

pub const FILE: &str = "geoip-country.csv";
pub const ATTRIBUTION: &str = "IP geolocation by DB-IP.com (CC BY 4.0)";
const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Default)]
pub struct Db {
    v4: Vec<(u32, u32, [u8; 2])>,
    v6: Vec<(u128, u128, [u8; 2])>,
}

fn cc(s: &str) -> Option<[u8; 2]> {
    let b = s.trim().as_bytes();
    (b.len() == 2 && b.iter().all(u8::is_ascii_alphabetic))
        .then(|| [b[0].to_ascii_uppercase(), b[1].to_ascii_uppercase()])
}

impl Db {
    pub fn parse(text: &str) -> Db {
        let mut db = Db::default();
        for line in text.lines() {
            let mut f = line.split(',');
            let (Some(a), Some(b), Some(c)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            let (a, b) = (a.trim().trim_matches('"'), b.trim().trim_matches('"'));
            let Some(c) = cc(c.trim_matches('"')) else {
                continue;
            };
            if c == *b"ZZ" {
                continue;
            }
            match (a.parse::<IpAddr>(), b.parse::<IpAddr>()) {
                (Ok(IpAddr::V4(x)), Ok(IpAddr::V4(y))) if u32::from(x) <= u32::from(y) => {
                    db.v4.push((x.into(), y.into(), c))
                }
                (Ok(IpAddr::V6(x)), Ok(IpAddr::V6(y))) if u128::from(x) <= u128::from(y) => {
                    db.v6.push((x.into(), y.into(), c))
                }
                _ => {}
            }
        }
        db.v4.sort_unstable_by_key(|r| r.0);
        db.v6.sort_unstable_by_key(|r| r.0);
        db
    }

    pub fn is_empty(&self) -> bool {
        self.v4.is_empty() && self.v6.is_empty()
    }

    pub fn lookup(&self, ip: IpAddr) -> Option<String> {
        let c = match ip {
            IpAddr::V4(a) => find(&self.v4, a.into()),
            IpAddr::V6(a) => match a.to_ipv4_mapped() {
                Some(m) => find(&self.v4, m.into()),
                None => find(&self.v6, a.into()),
            },
        }?;
        Some(String::from_utf8_lossy(&c).into_owned())
    }
}

fn find<T: Ord + Copy>(v: &[(T, T, [u8; 2])], x: T) -> Option<[u8; 2]> {
    let i = v.partition_point(|r| r.0 <= x);
    let r = v.get(i.checked_sub(1)?)?;
    (x <= r.1).then_some(r.2)
}

type Cached = Option<(SystemTime, std::sync::Arc<Db>)>;

fn cache() -> &'static Mutex<Cached> {
    static C: OnceLock<Mutex<Cached>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

/// Country code for `ip`, or None when no database is installed or it has no entry.
/// The file is reloaded when its modification time changes.
pub fn country(state_dir: Option<&Path>, ip: IpAddr) -> Option<String> {
    let path = state_dir?.join(FILE);
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let mtime = meta.modified().ok()?;
    let db = {
        let mut g = cache().lock().ok()?;
        match &*g {
            Some((t, d)) if *t == mtime => d.clone(),
            _ => {
                let text = std::fs::read_to_string(&path).ok()?;
                let d = std::sync::Arc::new(Db::parse(&text));
                *g = Some((mtime, d.clone()));
                d
            }
        }
    };
    db.lookup(ip)
}

fn one(state_dir: Option<&Path>, ip: Option<IpAddr>, why_none: &str) -> serde_json::Value {
    use serde_json::json;
    let Some(ip) = ip else {
        return json!({"country": null, "source": null, "reason": why_none});
    };
    if crate::target_guard::check_public(ip).is_err() {
        return json!({"country": null, "source": null, "reason": "address is not public (private, NAT or reserved)"});
    }
    match country(state_dir, ip) {
        Some(c) => json!({"country": c, "source": "offline_db"}),
        None => {
            let have = state_dir.is_some_and(|d| d.join(FILE).is_file());
            json!({"country": null, "source": null,
                   "reason": if have { "no entry in the offline database" } else { "offline database not installed" }})
        }
    }
}

/// This node's outbound IPv4 as the kernel would pick it. Connecting a UDP socket sends
/// no packet. A private result means the node is behind NAT.
fn outbound_ip() -> Option<IpAddr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    s.local_addr().ok().map(|a| a.ip())
}

/// `GET /v1/geo`: country only, never the address itself.
pub fn geo_json(state_dir: Option<&Path>, remote: Option<&str>) -> serde_json::Value {
    let peer_ip = remote
        .and_then(|r| r.parse::<std::net::SocketAddr>().ok())
        .map(|a| a.ip());
    serde_json::json!({
        "local": one(state_dir, outbound_ip(), "outbound address unknown"),
        "peer": one(state_dir, peer_ip, "no peer address (not an IP, or this node does not dial)"),
        "attribution": ATTRIBUTION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str = "\"1.0.0.0\",\"1.0.0.255\",\"AU\"\n2.144.0.0,2.147.255.255,IR\n\
5.0.0.0,4.0.0.0,XX\nbad line\n8.8.8.0,8.8.8.255,ZZ\n2a01:4f8::,2a01:4f8:ffff:ffff:ffff:ffff:ffff:ffff,de\n";

    #[test]
    fn lookup_and_bad_lines() {
        let d = Db::parse(CSV);
        let l = |s: &str| d.lookup(s.parse().unwrap());
        assert_eq!(l("1.0.0.7").as_deref(), Some("AU"));
        assert_eq!(l("2.145.1.1").as_deref(), Some("IR"));
        assert_eq!(l("2.148.0.0"), None);
        assert_eq!(l("0.255.255.255"), None);
        assert_eq!(l("5.0.0.1"), None); // reversed range skipped
        assert_eq!(l("8.8.8.8"), None); // ZZ means unknown
        assert_eq!(l("2a01:4f8::1").as_deref(), Some("DE"));
        assert_eq!(l("::ffff:1.0.0.1").as_deref(), Some("AU"));
        assert!(Db::parse("").is_empty());
    }

    #[test]
    fn geo_json_shape_and_reasons() {
        let g = geo_json(None, Some("10.0.0.1:443"));
        assert!(g["peer"]["country"].is_null());
        assert!(g["peer"]["reason"].as_str().unwrap().contains("not public"));
        let g = geo_json(None, Some("example.com:443"));
        assert!(g["peer"]["reason"].as_str().unwrap().contains("no peer"));
        let g = geo_json(None, Some("1.0.0.1:443"));
        assert_eq!(g["peer"]["reason"], "offline database not installed");
        assert!(g["local"].get("country").is_some());
        assert!(g["attribution"].as_str().unwrap().contains("DB-IP"));
    }

    /// Real DB-IP file: `BAFT_GEOIP_CSV=/path cargo test geoip_real -- --ignored`.
    #[test]
    #[ignore]
    fn geoip_real_file() {
        let p = std::env::var("BAFT_GEOIP_CSV").unwrap();
        let t = std::time::Instant::now();
        let d = Db::parse(&std::fs::read_to_string(p).unwrap());
        eprintln!("parsed in {:?}", t.elapsed());
        assert_eq!(
            d.lookup("2.144.1.1".parse().unwrap()).as_deref(),
            Some("IR")
        );
        assert_eq!(d.lookup("8.8.8.8".parse().unwrap()).as_deref(), Some("US"));
        assert!(d.lookup("2606:4700::1111".parse().unwrap()).is_some());
    }

    #[test]
    fn file_missing_is_unknown() {
        assert_eq!(country(None, "1.0.0.1".parse().unwrap()), None);
        let d = std::env::temp_dir().join("baft-geo-none");
        assert_eq!(country(Some(&d), "1.0.0.1".parse().unwrap()), None);
    }
}
