//! Read-only firewall visibility (ADR-0030, B3).
//!
//! The agent runs as an unprivileged user without `CAP_NET_ADMIN` and never lists or changes a
//! firewall. Instead the agent drops a request file `CONTROL_DIR/NAME.firewall` (one TCP port,
//! digits only); the root path unit `baft-tunnel-control-firewall@NAME.path` runs exactly
//! `baft-tunnel firewall-read-helper --instance NAME`. That helper runs one fixed read command
//! (`nft list ruleset`, else `iptables-save`), reduces the text to a small classification for the
//! tunnel port (no rule text, no addresses) and writes `RESULT_DIR/NAME.json`, a root-owned,
//! agent-readable file in a directory the agent cannot write. It never mutates a rule. Anything it
//! cannot decide stays `complex`/`unavailable`, which the agent reports as Unknown.
//!
//! Scope: IPv4 path only (nft families `ip` and `inet`, iptables IPv4), TCP, the INPUT hook.

use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const REQUEST_SUFFIX: &str = "firewall";
pub const RESULT_DIR: &str = "/run/baft-tunnel/firewall";
pub const WAIT: Duration = Duration::from_secs(3);
const MAX_RULESET_BYTES: usize = 4 * 1024 * 1024;
const MAX_LISTED_SETS: usize = 32;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Snapshot {
    pub backend: &'static str,
    pub input_chains: u32,
    pub policy_drop: bool,
    pub port_accept: u32,
    pub port_accept_scoped: u32,
    pub port_block: u32,
    pub other_block: u32,
    pub jumps: u32,
    pub port_by_set: bool,
    /// (table, set name, element count): names and counts only, never addresses.
    pub sets: Vec<(String, String, u32)>,
}

/// Does `rest` (text after `dport`) name `port`? `None` means it cannot be decided (a set).
fn dport_match(rest: &str, port: u16) -> Option<bool> {
    let rest = rest.trim_start();
    let list: &str = if let Some(b) = rest.strip_prefix('{') {
        b.split('}').next().unwrap_or("")
    } else if rest.starts_with('@') {
        return None;
    } else {
        rest.split_whitespace().next().unwrap_or("")
    };
    let mut any = false;
    for item in list.split(',') {
        let item = item.trim().trim_start_matches("!=").trim();
        if item.is_empty() {
            continue;
        }
        any = true;
        let hit = match item.split_once('-') {
            Some((a, b)) => matches!((a.trim().parse::<u16>(), b.trim().parse::<u16>()),
                (Ok(a), Ok(b)) if a <= port && port <= b),
            None => item.parse::<u16>() == Ok(port),
        };
        if hit {
            return Some(true);
        }
    }
    if any { Some(false) } else { None }
}

fn count_elements(text: &str) -> u32 {
    let t = text.trim().trim_end_matches(',');
    if t.is_empty() {
        0
    } else {
        t.split(',').count() as u32
    }
}

impl Snapshot {
    fn rule(&mut self, line: &str, in_input: bool, port: u16) {
        let l = line.trim();
        let (verdict_drop, verdict_accept, jump) = {
            let last = l.split_whitespace().last().unwrap_or("");
            let words: Vec<&str> = l.split_whitespace().collect();
            let jumped = words.iter().any(|w| matches!(*w, "jump" | "goto"))
                || (l.contains(" -j ")
                    && !l.ends_with("ACCEPT")
                    && !l.ends_with("DROP")
                    && !l.contains("-j REJECT")
                    && !l.ends_with("RETURN")
                    && !l.contains("-j LOG"));
            let drop = last == "drop"
                || l.ends_with("-j DROP")
                || last == "DROP"
                || words.contains(&"reject")
                || l.contains("-j REJECT");
            (drop, last == "accept" || l.ends_with("-j ACCEPT"), jumped)
        };
        let mut port_rule = None;
        for key in ["tcp dport", "--dport"] {
            if let Some(i) = l.find(key) {
                let proto_ok = key != "--dport" || l.contains("-p tcp");
                if proto_ok {
                    port_rule = Some(dport_match(&l[i + key.len()..], port));
                    break;
                }
            }
        }
        match port_rule {
            Some(None) => self.port_by_set = true,
            Some(Some(true)) => {
                if verdict_drop {
                    self.port_block += 1;
                } else if verdict_accept {
                    self.port_accept += 1;
                    if ["saddr", "iif", "-s ", "-i ", "iifname"]
                        .iter()
                        .any(|k| l.contains(k))
                    {
                        self.port_accept_scoped += 1;
                    }
                }
            }
            _ => {
                if in_input && verdict_drop && port_rule.is_none() {
                    self.other_block += 1;
                }
                if in_input && jump {
                    self.jumps += 1;
                }
            }
        }
    }
}

/// Classify `nft list ruleset` text for TCP `port` on the IPv4 path.
pub fn parse_nft(text: &str, port: u16) -> Snapshot {
    let mut s = Snapshot {
        backend: "nft",
        ..Snapshot::default()
    };
    let (mut table_ok, mut table, mut in_chain, mut in_input) =
        (false, String::new(), false, false);
    let mut set: Option<(String, String)> = None; // (name, collected text)
    for raw in text.lines() {
        let l = raw.trim();
        if let Some(rest) = l.strip_prefix("table ") {
            let mut w = rest.split_whitespace();
            let fam = w.next().unwrap_or("");
            table = w.next().unwrap_or("").to_string();
            table_ok = matches!(fam, "ip" | "inet");
            in_chain = false;
            continue;
        }
        if !table_ok {
            continue;
        }
        if let Some(rest) = l.strip_prefix("set ") {
            set = Some((
                rest.split_whitespace().next().unwrap_or("").to_string(),
                String::new(),
            ));
            continue;
        }
        if let Some((name, buf)) = set.as_mut() {
            if l == "}" {
                let body = buf
                    .split_once("elements = {")
                    .map_or("", |(_, r)| r.split('}').next().unwrap_or(""));
                s.sets
                    .push((table.clone(), name.clone(), count_elements(body)));
                set = None;
            } else {
                buf.push_str(l);
                buf.push(' ');
            }
            continue;
        }
        if l.starts_with("chain ") {
            in_chain = true;
            in_input = false;
            continue;
        }
        if in_chain && l.starts_with("type ") {
            if l.contains("hook input") && l.starts_with("type filter") {
                in_input = true;
                s.input_chains += 1;
                if l.contains("policy drop") {
                    s.policy_drop = true;
                }
            }
            continue;
        }
        if l == "}" {
            in_chain = false;
            in_input = false;
            continue;
        }
        if in_chain && !l.is_empty() {
            s.rule(l, in_input, port);
        }
    }
    s
}

/// Classify `iptables-save` text (IPv4 filter table) for TCP `port`.
pub fn parse_iptables(text: &str, port: u16) -> Snapshot {
    let mut s = Snapshot {
        backend: "iptables",
        ..Snapshot::default()
    };
    let mut filter = false;
    for raw in text.lines() {
        let l = raw.trim();
        if let Some(t) = l.strip_prefix('*') {
            filter = t == "filter";
        } else if filter {
            if let Some(p) = l.strip_prefix(":INPUT ") {
                s.input_chains = 1;
                s.policy_drop = p.starts_with("DROP");
            } else if let Some(r) = l.strip_prefix("-A ") {
                let input = r.starts_with("INPUT ");
                s.rule(l, input, port);
            }
        }
    }
    s
}

/// Reduce a snapshot to one verdict. Only `open`, `allowed` and `blocked` are decisions.
pub fn verdict(s: &Snapshot) -> &'static str {
    if s.backend == "none" {
        return "unavailable";
    }
    if s.port_by_set {
        return "complex";
    }
    if s.port_block > 0 {
        return "blocked_rule";
    }
    let restricting = s.policy_drop || s.other_block > 0 || s.jumps > 0;
    if s.input_chains == 0 || !restricting {
        return "open";
    }
    if s.other_block > 0 || s.jumps > 0 {
        return "complex";
    }
    // Default policy drop and every rule in the hooked chain is an accept or neutral.
    if s.port_accept > s.port_accept_scoped {
        "allowed"
    } else if s.port_accept_scoped > 0 {
        "complex"
    } else {
        "no_rule"
    }
}

fn sets_digest(sets: &[(String, String, u32)]) -> String {
    let mut lines: Vec<String> = sets
        .iter()
        .map(|(t, n, c)| format!("{t}/{n}/{c}"))
        .collect();
    lines.sort();
    hex::encode(Sha256::digest(lines.join("\n").as_bytes()))
}

pub fn document(s: &Snapshot, port: u16, now: u64) -> Value {
    let listed: Vec<Value> = s
        .sets
        .iter()
        .take(MAX_LISTED_SETS)
        .map(|(t, n, c)| json!({"table": t, "set": n, "elements": c}))
        .collect();
    json!({
        "v": 1, "generated_unix": now, "port": port, "protocol": "tcp", "scope": "ipv4-input",
        "backend": s.backend, "verdict": verdict(s),
        "input_chains": s.input_chains, "policy_drop": s.policy_drop,
        "port_accept_rules": s.port_accept, "port_block_rules": s.port_block,
        "other_block_rules": s.other_block, "jump_rules": s.jumps,
        "sets": listed, "sets_digest": sets_digest(&s.sets),
        "mutated": false
    })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn valid_name(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && b.len() <= 63
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
}

fn read_request(path: &Path) -> Result<u16> {
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    if !f.metadata()?.is_file() {
        bail!("request is not a regular file");
    }
    let mut buf = Vec::new();
    Read::take(&mut f, 33).read_to_end(&mut buf)?;
    if buf.len() > 32 {
        bail!("request too large");
    }
    let s = std::str::from_utf8(&buf)?.trim();
    if s.is_empty() || s.len() > 5 || !s.bytes().all(|b| b.is_ascii_digit()) {
        bail!("request must be one TCP port");
    }
    match s.parse::<u16>() {
        Ok(p) if p > 0 => Ok(p),
        _ => bail!("port out of range"),
    }
}

/// Runs `argv` (absolute path, fixed arguments) and returns stdout, bounded.
fn capture(argv: &[&str]) -> Option<String> {
    if !Path::new(argv[0]).is_file() {
        return None;
    }
    let out = std::process::Command::new(argv[0])
        .args(&argv[1..])
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.len() > MAX_RULESET_BYTES {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// The two fixed read commands. Neither can change a rule.
pub fn read_system(port: u16) -> Snapshot {
    if let Some(t) = capture(&["/usr/sbin/nft", "list", "ruleset"]) {
        return parse_nft(&t, port);
    }
    if let Some(t) = capture(&["/usr/sbin/iptables-save"]) {
        return parse_iptables(&t, port);
    }
    Snapshot {
        backend: "none",
        ..Snapshot::default()
    }
}

fn write_result(result_dir: &Path, name: &str, doc: &Value) -> Result<()> {
    let meta = std::fs::symlink_metadata(result_dir)
        .with_context(|| format!("result directory {}", result_dir.display()))?;
    if !meta.is_dir() || meta.file_type().is_symlink() || meta.mode() & 0o022 != 0 {
        bail!(
            "result directory {} is not a private real directory",
            result_dir.display()
        );
    }
    let tmp = result_dir.join(format!(".{name}.tmp"));
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o640)
        .open(&tmp)?;
    // SAFETY: fchown on an open descriptor with plain integers.
    unsafe {
        libc::fchown(
            std::os::unix::io::AsRawFd::as_raw_fd(&f),
            u32::MAX,
            meta.gid(),
        )
    };
    f.write_all(&serde_json::to_vec(doc)?)?;
    f.sync_all()?;
    std::fs::rename(&tmp, result_dir.join(format!("{name}.json")))?;
    Ok(())
}

/// Root helper body: read and remove the request, read the firewall once, write the result.
pub fn run(
    name: &str,
    control_dir: &Path,
    result_dir: &Path,
    reader: &dyn Fn(u16) -> Snapshot,
) -> Result<Value> {
    if !valid_name(name) {
        bail!("invalid instance name");
    }
    let req = control_dir.join(format!("{name}.{REQUEST_SUFFIX}"));
    let port = read_request(&req);
    let _ = std::fs::remove_file(&req);
    let port = port?;
    let doc = document(&reader(port), port, now());
    write_result(result_dir, name, &doc)?;
    Ok(doc)
}

/// Agent side: ask the helper and wait for a fresh result for this port. `Err` carries a reason
/// code; every error means the firewall stays Unknown.
pub fn request(
    name: &str,
    port: u16,
    control_dir: &Path,
    result_dir: &Path,
    wait: Duration,
) -> Result<Value, &'static str> {
    if !valid_name(name) {
        return Err("bad_name");
    }
    let started = now();
    let req = control_dir.join(format!("{name}.{REQUEST_SUFFIX}"));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o660)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&req)
        .map_err(|_| "helper_missing")?;
    f.write_all(port.to_string().as_bytes())
        .map_err(|_| "helper_missing")?;
    drop(f);
    let deadline = Instant::now() + wait;
    let out = result_dir.join(format!("{name}.json"));
    while Instant::now() < deadline {
        if let Ok(bytes) = std::fs::read(&out) {
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                if v["port"] == port
                    && v["generated_unix"]
                        .as_u64()
                        .is_some_and(|g| g + 1 >= started)
                {
                    return Ok(v);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = std::fs::remove_file(&req);
    Err("helper_missing")
}

/// Map a helper result to a Preflight check `(result, code, evidence, params)`.
pub fn to_check(v: &Value) -> (&'static str, String, String, Value) {
    let port = v["port"].as_u64().unwrap_or(0);
    let params = json!({
        "port": port, "backend": v["backend"], "scope": v["scope"],
        "sets_digest": v["sets_digest"],
    });
    let (res, code, text) = match v["verdict"].as_str().unwrap_or("") {
        "open" => ("pass", "pf.firewall.open", format!("no inbound IPv4 filter rule restricts TCP port {port} ({}; read-only)", v["backend"].as_str().unwrap_or("?"))),
        "allowed" => ("pass", "pf.firewall.allowed", format!("default input policy is drop and a rule accepts TCP port {port} (read-only; the rule's source scope is unscoped)")),
        "no_rule" => ("blocked", "pf.firewall.no_rule", format!("default input policy is drop and no rule accepts TCP port {port}")),
        "blocked_rule" => ("blocked", "pf.firewall.blocked_rule", format!("a rule drops or rejects TCP port {port}")),
        "complex" => ("unknown", "pf.firewall.complex", "the firewall rules are too complex to decide safely (jumps, sets or scoped rules); not verified".to_string()),
        _ => ("unknown", "pf.firewall.unavailable", "no firewall reader (nft or iptables-save) is available to the helper; not verified".to_string()),
    };
    (res, code.to_string(), text, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const NFT_DROP_ACCEPT: &str = "table inet filter {\n\tset blocked4 {\n\t\ttype ipv4_addr\n\t\telements = { 192.0.2.1, 192.0.2.2,\n\t\t\t     192.0.2.3 }\n\t}\n\tchain input {\n\t\ttype filter hook input priority filter; policy drop;\n\t\tct state established,related accept\n\t\ttcp dport 22 accept\n\t\ttcp dport { 8443, 9000-9100 } accept\n\t}\n}\n";

    #[test]
    fn allowed_when_policy_drop_and_port_accepted() {
        let s = parse_nft(NFT_DROP_ACCEPT, 9050);
        assert_eq!(verdict(&s), "allowed");
        assert_eq!(verdict(&parse_nft(NFT_DROP_ACCEPT, 8443)), "allowed");
        assert_eq!(verdict(&parse_nft(NFT_DROP_ACCEPT, 4444)), "no_rule");
        assert_eq!(s.sets, vec![("filter".into(), "blocked4".into(), 3)]);
    }

    #[test]
    fn open_when_no_input_filtering_and_ipv6_only_rules_are_ignored() {
        let t = "table ip6 filter {\n\tchain input {\n\t\ttype filter hook input priority filter; policy drop;\n\t}\n}\n";
        assert_eq!(verdict(&parse_nft(t, 443)), "open");
        let permissive = "table ip filter {\n\tchain input {\n\t\ttype filter hook input priority filter; policy accept;\n\t}\n}\n";
        assert_eq!(verdict(&parse_nft(permissive, 443)), "open");
    }

    #[test]
    fn explicit_block_wins_and_sets_or_jumps_are_complex() {
        let b = "table ip f {\n\tchain input {\n\t\ttype filter hook input priority filter; policy accept;\n\t\ttcp dport 443 drop\n\t}\n}\n";
        assert_eq!(verdict(&parse_nft(b, 443)), "blocked_rule");
        let set = "table ip f {\n\tchain input {\n\t\ttype filter hook input priority filter; policy drop;\n\t\ttcp dport @ports accept\n\t}\n}\n";
        assert_eq!(verdict(&parse_nft(set, 443)), "complex");
        let jump = "table ip f {\n\tchain input {\n\t\ttype filter hook input priority filter; policy drop;\n\t\tjump ufw-user-input\n\t}\n\tchain ufw-user-input {\n\t\ttcp dport 443 accept\n\t}\n}\n";
        assert_eq!(verdict(&parse_nft(jump, 443)), "complex");
        let scoped = "table ip f {\n\tchain input {\n\t\ttype filter hook input priority filter; policy drop;\n\t\tip saddr 198.51.100.7 tcp dport 443 accept\n\t}\n}\n";
        assert_eq!(verdict(&parse_nft(scoped, 443)), "complex");
        let other = "table ip f {\n\tchain input {\n\t\ttype filter hook input priority filter; policy accept;\n\t\tip saddr 203.0.113.0/24 drop\n\t}\n}\n";
        assert_eq!(verdict(&parse_nft(other, 443)), "complex");
    }

    #[test]
    fn iptables_save_is_classified_the_same_way() {
        let t = "*filter\n:INPUT DROP [0:0]\n:FORWARD DROP [0:0]\n-A INPUT -p tcp -m tcp --dport 9050 -j ACCEPT\nCOMMIT\n";
        assert_eq!(verdict(&parse_iptables(t, 9050)), "allowed");
        assert_eq!(verdict(&parse_iptables(t, 9051)), "no_rule");
        let nat = "*nat\n:PREROUTING ACCEPT [0:0]\nCOMMIT\n";
        assert_eq!(verdict(&parse_iptables(nat, 9050)), "open");
    }

    #[test]
    fn digest_ignores_order_and_never_lists_addresses() {
        let a = vec![
            ("t".to_string(), "a".to_string(), 2),
            ("t".to_string(), "b".to_string(), 1),
        ];
        let mut b = a.clone();
        b.reverse();
        assert_eq!(sets_digest(&a), sets_digest(&b));
        let s = Snapshot {
            backend: "nft",
            sets: a,
            ..Snapshot::default()
        };
        assert!(!document(&s, 1, 0).to_string().contains("192."));
        assert_eq!(document(&s, 1, 0)["mutated"], false);
    }

    struct Tmp(std::path::PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn dirs(tag: &str) -> (Tmp, std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("baft-fw-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (c, r) = (root.join("control"), root.join("fw"));
        std::fs::create_dir_all(&c).unwrap();
        std::fs::create_dir_all(&r).unwrap();
        std::fs::set_permissions(&r, std::fs::Permissions::from_mode(0o750)).unwrap();
        (Tmp(root), c, r)
    }

    #[test]
    fn helper_reads_request_writes_result_and_removes_request() {
        let (_t, c, r) = dirs("ok");
        std::fs::write(c.join("n1.firewall"), "9050").unwrap();
        let doc = run("n1", &c, &r, &|p| parse_nft(NFT_DROP_ACCEPT, p)).unwrap();
        assert_eq!(doc["verdict"], "allowed");
        assert!(!c.join("n1.firewall").exists());
        let back: Value =
            serde_json::from_slice(&std::fs::read(r.join("n1.json")).unwrap()).unwrap();
        assert_eq!(back["port"], 9050);
        assert_eq!(
            std::fs::metadata(r.join("n1.json")).unwrap().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn helper_refuses_bad_requests_and_links() {
        let (_t, c, r) = dirs("bad");
        for body in ["abc", "0", "70000", "22 && rm", ""] {
            std::fs::write(c.join("n1.firewall"), body).unwrap();
            assert!(
                run("n1", &c, &r, &|p| parse_nft("", p)).is_err(),
                "{body:?}"
            );
            assert!(!c.join("n1.firewall").exists());
        }
        std::os::unix::fs::symlink("/etc/passwd", c.join("n1.firewall")).unwrap();
        assert!(run("n1", &c, &r, &|p| parse_nft("", p)).is_err());
        assert!(run("../x", &c, &r, &|p| parse_nft("", p)).is_err());
        std::fs::set_permissions(&r, std::fs::Permissions::from_mode(0o770)).unwrap();
        std::fs::write(c.join("n1.firewall"), "22").unwrap();
        assert!(run("n1", &c, &r, &|p| parse_nft("", p)).is_err());
    }

    #[test]
    fn agent_gets_unknown_reason_when_no_helper_answers() {
        let (_t, c, r) = dirs("agent");
        assert_eq!(
            request("n1", 22, &c, &r, Duration::from_millis(300)).unwrap_err(),
            "helper_missing"
        );
        assert!(!c.join("n1.firewall").exists());
    }

    #[test]
    fn check_mapping_never_turns_complex_into_pass() {
        let v = json!({"port": 1, "verdict": "complex", "backend": "nft"});
        assert_eq!(to_check(&v).0, "unknown");
        assert_eq!(to_check(&json!({"verdict": "?"})).0, "unknown");
        assert_eq!(to_check(&json!({"verdict": "open", "port": 1})).0, "pass");
        assert_eq!(
            to_check(&json!({"verdict": "no_rule", "port": 1})).0,
            "blocked"
        );
    }
}
