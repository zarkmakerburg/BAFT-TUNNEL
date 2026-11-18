//! ADR-0052: pure rules for server-location and probe targets. No I/O.
#![allow(dead_code)] // used by the panel card in a later slice (ADR-0052)

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Why a typed address may not be looked up or probed.
#[derive(Debug, PartialEq, Eq)]
pub enum Refuse {
    NotPublic,
}

fn v4_public(a: Ipv4Addr) -> bool {
    let o = a.octets();
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_private()
        || a.is_link_local() // includes 169.254.169.254 metadata
        || a.is_broadcast()
        || a.is_multicast()
        || a.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (64..=127).contains(&o[1])) // CGNAT
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) // benchmarking
        || o[0] >= 240) // reserved
}

fn v6_public(a: Ipv6Addr) -> bool {
    if let Some(m) = a.to_ipv4_mapped() {
        return v4_public(m);
    }
    let s = a.segments();
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // ULA
        || (s[0] & 0xffc0) == 0xfe80 // link-local
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        || (s[0] == 0x64 && s[1] == 0xff9b)) // NAT64 can embed private v4
}

/// Accept only globally routable unicast addresses.
pub fn check_public(ip: IpAddr) -> Result<IpAddr, Refuse> {
    let ok = match ip {
        IpAddr::V4(a) => v4_public(a),
        IpAddr::V6(a) => v6_public(a),
    };
    if ok { Ok(ip) } else { Err(Refuse::NotPublic) }
}

/// Suggested role for the first setup question. `None` means no suggestion.
pub fn suggest_role(country: Option<&str>) -> Option<&'static str> {
    let c = country?;
    if c.len() != 2 || !c.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    if c.eq_ignore_ascii_case("IR") {
        Some("iran")
    } else {
        Some("abroad")
    }
}

/// TTL cache plus a per-hour budget. Time is passed in, so it is testable.
pub struct LookupGate {
    ttl_s: u64,
    per_target_per_hour: usize,
    global_per_hour: usize,
    cache: HashMap<IpAddr, (u64, String)>,
    log: Vec<(u64, IpAddr)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Gate {
    Cached(String),
    Allowed,
    Limited,
}

impl LookupGate {
    pub fn new(ttl_s: u64, per_target_per_hour: usize, global_per_hour: usize) -> Self {
        Self {
            ttl_s,
            per_target_per_hour,
            global_per_hour,
            cache: HashMap::new(),
            log: Vec::new(),
        }
    }

    /// Decide whether a fresh lookup may run now. Records it when allowed.
    pub fn ask(&mut self, ip: IpAddr, now: u64) -> Gate {
        if let Some((t, v)) = self.cache.get(&ip) {
            if now.saturating_sub(*t) < self.ttl_s {
                return Gate::Cached(v.clone());
            }
        }
        self.log.retain(|(t, _)| now.saturating_sub(*t) < 3600);
        let per = self.log.iter().filter(|(_, i)| *i == ip).count();
        if per >= self.per_target_per_hour || self.log.len() >= self.global_per_hour {
            return Gate::Limited;
        }
        self.log.push((now, ip));
        Gate::Allowed
    }

    pub fn store(&mut self, ip: IpAddr, now: u64, value: String) {
        self.cache.insert(ip, (now, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn refuses_non_public() {
        for s in [
            "127.0.0.1",
            "10.0.0.1",
            "192.168.1.1",
            "172.16.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "240.0.0.1",
            "::1",
            "::",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:10.0.0.1",
            "::ffff:127.0.0.1",
            "64:ff9b::a00:1",
        ] {
            assert_eq!(check_public(ip(s)), Err(Refuse::NotPublic), "{s}");
        }
    }

    #[test]
    fn accepts_public() {
        for s in ["8.8.8.8", "5.160.0.1", "2606:4700::1111", "::ffff:8.8.8.8"] {
            assert!(check_public(ip(s)).is_ok(), "{s}");
        }
    }

    #[test]
    fn role_suggestion_is_unknown_when_unknown() {
        assert_eq!(suggest_role(None), None);
        assert_eq!(suggest_role(Some("")), None);
        assert_eq!(suggest_role(Some("??")), None);
        assert_eq!(suggest_role(Some("ir")), Some("iran"));
        assert_eq!(suggest_role(Some("DE")), Some("abroad"));
    }

    #[test]
    fn cache_and_budget() {
        let mut g = LookupGate::new(600, 2, 3);
        let a = ip("8.8.8.8");
        assert_eq!(g.ask(a, 0), Gate::Allowed);
        g.store(a, 0, "US".into());
        assert_eq!(g.ask(a, 100), Gate::Cached("US".into()));
        assert_eq!(g.ask(a, 700), Gate::Allowed);
        assert_eq!(g.ask(a, 800), Gate::Limited); // per-target budget
        let b = ip("1.1.1.1");
        assert_eq!(g.ask(b, 900), Gate::Allowed); // global now 3
        assert_eq!(g.ask(ip("9.9.9.9"), 901), Gate::Limited);
        assert_eq!(g.ask(ip("9.9.9.9"), 5000), Gate::Allowed); // window slid
    }
}
