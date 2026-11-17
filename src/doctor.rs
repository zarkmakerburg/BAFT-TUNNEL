//! `baft-tunnel doctor` (ADR-0042): read-only checks of one config file on this host.
//! Results are PASS / WARNING / FAIL / UNKNOWN. UNKNOWN means "tried, inconclusive" and is never
//! folded into PASS. Anything that needs a live agent (integrity, link health) is listed under
//! `not_checked`, not reported as a result. No secrets are read or printed, nothing is changed.

use std::net::{SocketAddr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pass,
    Warning,
    Fail,
    Unknown,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warning => "WARNING",
            Self::Fail => "FAIL",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Check {
    pub id: String,
    pub status: Status,
    pub detail: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub checks: Vec<Check>,
    pub not_checked: Vec<String>,
}

const CERT_WARNING_SECONDS: i64 = 30 * 24 * 3600;
const CRL_WARNING_SECONDS: i64 = 72 * 3600;

impl Report {
    fn add(&mut self, id: &str, status: Status, detail: impl Into<String>) {
        self.checks.push(Check {
            id: id.to_owned(),
            status,
            detail: detail.into(),
        });
    }

    /// 1 when any check failed, else 2 when any is unknown, else 0 (warnings do not fail), so a
    /// script can never read an inconclusive run as healthy.
    pub fn exit_code(&self) -> u8 {
        if self.checks.iter().any(|c| c.status == Status::Fail) {
            1
        } else if self.checks.iter().any(|c| c.status == Status::Unknown) {
            2
        } else {
            0
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "application": "baft-tunnel",
            "exit_code": self.exit_code(),
            "checks": self.checks.iter().map(|c| json!({
                "id": c.id, "status": c.status.as_str(), "detail": c.detail
            })).collect::<Vec<_>>(),
            "not_checked": self.not_checked,
        })
    }

    pub fn human(&self) -> String {
        let mut out = String::new();
        for c in &self.checks {
            out.push_str(&format!(
                "{:<8} {:<22} {}\n",
                c.status.as_str(),
                c.id,
                c.detail
            ));
        }
        for n in &self.not_checked {
            out.push_str(&format!("-        not checked            {n}\n"));
        }
        out.push_str(&format!("exit code {}\n", self.exit_code()));
        out
    }
}

fn expiry_status(seconds: i64, warn_below: i64) -> Status {
    if seconds <= 0 {
        Status::Fail
    } else if seconds < warn_below {
        Status::Warning
    } else {
        Status::Pass
    }
}

fn check_key_permissions(report: &mut Report, key: &Path) {
    match std::fs::metadata(key) {
        Ok(meta) if meta.permissions().mode() & 0o077 == 0 => report.add(
            "key.permissions",
            Status::Pass,
            "private key is not group/other accessible",
        ),
        Ok(_) => report.add(
            "key.permissions",
            Status::Fail,
            "private key is readable by group or others; run chmod 600 on it",
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => report.add(
            "key.permissions",
            Status::Fail,
            "private key file does not exist",
        ),
        Err(e) => report.add(
            "key.permissions",
            Status::Unknown,
            format!("cannot inspect the private key as this user ({})", e.kind()),
        ),
    }
}

fn check_file_exists(report: &mut Report, id: &str, path: &Path) {
    if path.is_file() {
        report.add(id, Status::Pass, "file present");
    } else {
        report.add(id, Status::Fail, "file is missing");
    }
}

/// Run every offline check; `network` adds one TCP connect per dialed remote (no data is sent).
pub fn run(path: &Path, network: bool) -> Report {
    let mut report = Report::default();
    let config = match config::load(path) {
        Ok(c) => {
            report.add("config.parse", Status::Pass, "JSON parsed, no unknown keys");
            c
        }
        Err(e) => {
            report.add("config.parse", Status::Fail, format!("{e:#}"));
            return report;
        }
    };
    match config.validate_public() {
        Ok(()) => report.add(
            "config.validate",
            Status::Pass,
            "identities, sockets and engine settings are valid",
        ),
        Err(e) => report.add("config.validate", Status::Fail, format!("{e:#}")),
    }
    let (local, peer, tunnel) = config.ids();
    report.add(
        "node.identity",
        Status::Pass,
        format!(
            "{local} -> {peer}, tunnel {tunnel}, {} / {}",
            config.direction_name(),
            config.engine_name()
        ),
    );
    report.add(
        "protocol.version",
        Status::Pass,
        format!(
            "protocol v{}; both nodes must use the same version (strict, no mixed mode)",
            config.protocol_version()
        ),
    );

    let inputs = config.doctor_inputs();
    match (inputs.key, inputs.key_credential) {
        (Some(key), _) => check_key_permissions(&mut report, key),
        (None, true) => report
            .not_checked
            .push("private key custody (systemd credential, not inspectable here)".into()),
        (None, false) => {}
    }
    if inputs.uses_certificates {
        for (id, file) in [
            ("tls.ca", inputs.ca),
            ("tls.crl", inputs.crl),
            ("tls.cert", inputs.cert),
        ] {
            check_file_exists(&mut report, id, file);
        }
        match crate::tls::certificate_seconds_remaining(inputs.cert) {
            Ok(s) => report.add(
                "cert.expiry",
                expiry_status(s, CERT_WARNING_SECONDS),
                if s <= 0 {
                    "node certificate has expired".to_owned()
                } else {
                    format!("node certificate valid for {} more days", s / 86400)
                },
            ),
            Err(e) => report.add("cert.expiry", Status::Unknown, format!("{e:#}")),
        }
        match crate::tls::crl_seconds_remaining(inputs.crl) {
            Ok(s) => report.add(
                "crl.expiry",
                expiry_status(s, CRL_WARNING_SECONDS),
                if s <= 0 {
                    "revocation list has expired; TLS fails closed".to_owned()
                } else {
                    format!("revocation list valid for {} more hours", s / 3600)
                },
            ),
            Err(e) => report.add("crl.expiry", Status::Unknown, format!("{e:#}")),
        }
    }
    if !inputs.remotes.is_empty() {
        report.add(
            "links",
            Status::Pass,
            format!("{} link(s), in priority order", inputs.remotes.len()),
        );
        if network {
            for remote in &inputs.remotes {
                let id = format!("reach.{remote}");
                match remote.parse::<SocketAddr>() {
                    Ok(addr) => match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
                        Ok(_) => {
                            report.add(&id, Status::Pass, "TCP connect succeeded (no data sent)")
                        }
                        Err(e) => report.add(
                            &id,
                            Status::Fail,
                            format!("TCP connect failed: {}", e.kind()),
                        ),
                    },
                    Err(_) => report.add(&id, Status::Fail, "not an IP:port address"),
                }
            }
        } else {
            report
                .not_checked
                .push("network reachability (add --network)".into());
        }
    }
    report
        .not_checked
        .push("integrity evidence (needs the live agent)".into());
    report
        .not_checked
        .push("link health, RTT and loss (needs the live agent)".into());
    report
        .not_checked
        .push("agent control socket security (checked by the agent)".into());
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("baft-doctor-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn exit_code_never_reads_unknown_as_healthy() {
        let mut r = Report::default();
        r.add("a", Status::Pass, "");
        r.add("b", Status::Warning, "");
        assert_eq!(r.exit_code(), 0, "warnings alone do not fail");
        r.add("c", Status::Unknown, "");
        assert_eq!(r.exit_code(), 2);
        r.add("d", Status::Fail, "");
        assert_eq!(r.exit_code(), 1, "fail wins over unknown");
    }

    #[test]
    fn expiry_thresholds() {
        assert_eq!(expiry_status(-5, 100), Status::Fail);
        assert_eq!(expiry_status(0, 100), Status::Fail);
        assert_eq!(expiry_status(50, 100), Status::Warning);
        assert_eq!(expiry_status(500, 100), Status::Pass);
    }

    #[test]
    fn key_permissions_pass_fail_missing() {
        let dir = temp("key");
        let key = dir.join("k");
        std::fs::write(&key, b"x").unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut r = Report::default();
        check_key_permissions(&mut r, &key);
        assert_eq!(r.checks[0].status, Status::Pass);
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o640)).unwrap();
        let mut r = Report::default();
        check_key_permissions(&mut r, &key);
        assert_eq!(r.checks[0].status, Status::Fail);
        let mut r = Report::default();
        check_key_permissions(&mut r, &dir.join("absent"));
        assert_eq!(r.checks[0].status, Status::Fail);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bad_config_fails_fast_and_lists_nothing_as_passed() {
        let dir = temp("cfg");
        let missing = run(&dir.join("nope.json"), false);
        assert_eq!(missing.exit_code(), 1);
        assert_eq!(missing.checks.len(), 1);
        let bad = dir.join("bad.json");
        std::fs::write(&bad, br#"{"mode":"forward","surprise":1}"#).unwrap();
        let r = run(&bad, false);
        assert_eq!(r.exit_code(), 1);
        assert_eq!(r.checks[0].id, "config.parse");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn json_and_human_output_agree_and_expose_not_checked() {
        let mut r = Report::default();
        r.add("x", Status::Warning, "careful");
        r.not_checked.push("integrity".into());
        let j = r.to_json();
        assert_eq!(j["exit_code"], 0);
        assert_eq!(j["checks"][0]["status"], "WARNING");
        assert_eq!(j["not_checked"][0], "integrity");
        let h = r.human();
        assert!(h.contains("WARNING") && h.contains("not checked") && h.contains("exit code 0"));
    }
}
