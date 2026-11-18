//! Fleet start-up (ADR-0045 F1, read-only, Preview): reads the optional `--fleet-config` file and
//! starts the hub listener and/or the member reporter inside the agent. Off unless the flag is
//! given. The file holds only certificate file paths and addresses, never key material. Nothing
//! here can start, stop or change a tunnel: the hub only stores reports, the member only sends one.
#![allow(dead_code)]

use crate::agent::Ctx;
use crate::fleet_report::ReporterConfig;
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::watch;

const MAX_CONFIG_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetConfig {
    pub hub: Option<HubSection>,
    pub member: Option<MemberSection>,
}

/// This host acts as the hub: its own `fleet-hub` certificate and the CA that issued the nodes.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HubSection {
    pub listen: String,
    pub ca: PathBuf,
    pub crl: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// This host reports to a hub with its `fleet-node` certificate.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSection {
    pub hub_addr: String,
    pub hub_name: String,
    pub hub_id: String,
    pub ca: PathBuf,
    pub crl: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
}

fn valid_id(s: &str) -> bool {
    (1..=63).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn check_file(label: &str, p: &Path) -> Result<()> {
    ensure!(p.is_absolute(), "{label} must be an absolute path");
    ensure!(
        std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false),
        "{label} is not a readable file: {}",
        p.display()
    );
    Ok(())
}

impl FleetConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let meta = std::fs::symlink_metadata(path)
            .with_context(|| format!("cannot read fleet config {}", path.display()))?;
        ensure!(
            meta.file_type().is_file() && meta.len() <= MAX_CONFIG_BYTES,
            "fleet config must be a regular file of at most {MAX_CONFIG_BYTES} bytes"
        );
        let text = std::fs::read_to_string(path).context("read fleet config")?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self> {
        let cfg: Self = serde_json::from_str(text).context("fleet config is not valid")?;
        if cfg.hub.is_none() && cfg.member.is_none() {
            bail!("fleet config enables neither a hub nor a member");
        }
        if let Some(h) = &cfg.hub {
            h.listen
                .parse::<SocketAddr>()
                .context("fleet hub listen must be ip:port")?;
            for (l, p) in [
                ("hub ca", &h.ca),
                ("hub crl", &h.crl),
                ("hub cert", &h.cert),
                ("hub key", &h.key),
            ] {
                check_file(l, p)?;
            }
        }
        if let Some(m) = &cfg.member {
            ensure!(
                valid_id(&m.hub_id),
                "member hub_id must be [a-z0-9-]{{1,63}}"
            );
            ensure!(
                m.hub_addr
                    .rsplit_once(':')
                    .is_some_and(|(h, p)| !h.is_empty() && p.parse::<u16>().is_ok()),
                "member hub_addr must be host:port"
            );
            ensure!(!m.hub_name.is_empty(), "member hub_name is empty");
            for (l, p) in [
                ("member ca", &m.ca),
                ("member crl", &m.crl),
                ("member cert", &m.cert),
                ("member key", &m.key),
            ] {
                check_file(l, p)?;
            }
        }
        Ok(cfg)
    }
}

/// Start what the file asks for. The member needs the agent's `--instance` (its report names it).
/// Failures to bind or load certificates are returned so the unit fails visibly instead of
/// running without the Fleet role the operator configured.
pub async fn start(ctx: Arc<Ctx>, cfg: FleetConfig, instance: Option<&str>) -> Result<()> {
    // The tasks live as long as the agent process; shutdown is the process exit.
    let (_stop, rx) = watch::channel(false);
    std::mem::forget(_stop);
    if let Some(h) = cfg.hub {
        let acceptor = Arc::new(crate::tls::ReloadingAcceptor::new(
            &h.ca, &h.crl, &h.cert, &h.key,
        )?);
        let listener = TcpListener::bind(&h.listen)
            .await
            .with_context(|| format!("cannot bind fleet hub on {}", h.listen))?;
        eprintln!(
            "Fleet hub listening on {} (mutual TLS, read-only)",
            h.listen
        );
        tokio::spawn(crate::fleet_hub::serve(
            listener,
            acceptor,
            ctx.clone(),
            rx.clone(),
        ));
    }
    if let Some(m) = cfg.member {
        let instance = instance.context("a Fleet member needs the agent's --instance")?;
        // Fail now if the material does not load, not at the first report.
        crate::tls::client_config(&m.ca, &m.crl, &m.cert, &m.key)?;
        let rc = ReporterConfig {
            hub_addr: m.hub_addr,
            hub_name: m.hub_name,
            hub_id: m.hub_id,
            ca: m.ca,
            crl: m.crl,
            cert: m.cert,
            key: m.key,
            instance: instance.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        eprintln!(
            "Fleet member reporting to {} (outbound mutual TLS)",
            rc.hub_addr
        );
        let c = ctx.clone();
        tokio::spawn(crate::fleet_report::run(
            rc,
            move || {
                let c = c.clone();
                async move {
                    match &c.config {
                        Some(config) => crate::agent::status(&c, config).await,
                        None => serde_json::json!({"tunnel": null}),
                    }
                }
            },
            rx,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(dir: &Path) -> [String; 4] {
        let mk = |n: &str| {
            let p = dir.join(n);
            std::fs::write(&p, "x").unwrap();
            p.display().to_string()
        };
        [mk("ca.pem"), mk("ca.crl"), mk("c.pem"), mk("k.pem")]
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("baft-fleet-run-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn config_is_strict_and_off_by_default() {
        let d = tmp("cfg");
        let [ca, crl, cert, key] = files(&d);
        assert!(FleetConfig::parse("{}").is_err());
        assert!(FleetConfig::parse("not json").is_err());
        // Unknown fields are refused so a typo cannot silently weaken a setting.
        let typo = format!(
            r#"{{"hub":{{"listen":"127.0.0.1:1","ca":"{ca}","crl":"{crl}","cert":"{cert}","key":"{key}","oops":1}}}}"#
        );
        assert!(FleetConfig::parse(&typo).is_err());
        let hub = format!(
            r#"{{"hub":{{"listen":"127.0.0.1:18443","ca":"{ca}","crl":"{crl}","cert":"{cert}","key":"{key}"}}}}"#
        );
        assert!(FleetConfig::parse(&hub).unwrap().hub.is_some());
        let bad_listen = hub.replace("127.0.0.1:18443", "nope");
        assert!(FleetConfig::parse(&bad_listen).is_err());
        let relative = hub.replace(&ca, "ca.pem");
        assert!(FleetConfig::parse(&relative).is_err());
        let member = |id: &str, addr: &str| {
            format!(
                r#"{{"member":{{"hub_addr":"{addr}","hub_name":"hub.example","hub_id":"{id}","ca":"{ca}","crl":"{crl}","cert":"{cert}","key":"{key}"}}}}"#
            )
        };
        assert!(
            FleetConfig::parse(&member("hub-1", "hub.example:18443"))
                .unwrap()
                .member
                .is_some()
        );
        assert!(FleetConfig::parse(&member("Hub_1", "hub.example:18443")).is_err());
        assert!(FleetConfig::parse(&member("hub-1", "no-port")).is_err());
        assert!(FleetConfig::load(&d.join("missing.json")).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn member_without_instance_is_refused_and_missing_material_fails_start() {
        let d = tmp("start");
        let [ca, crl, cert, key] = files(&d);
        let member = format!(
            r#"{{"member":{{"hub_addr":"127.0.0.1:1","hub_name":"hub.example","hub_id":"hub-1","ca":"{ca}","crl":"{crl}","cert":"{cert}","key":"{key}"}}}}"#
        );
        let cfg = FleetConfig::parse(&member).unwrap();
        let ctx = Arc::new(Ctx::with_config(None, "t".repeat(40), None));
        assert!(start(ctx.clone(), cfg.clone(), None).await.is_err());
        // Placeholder files are not certificates: start fails visibly, not at the first report.
        assert!(start(ctx, cfg, Some("edge")).await.is_err());
        let _ = std::fs::remove_dir_all(d);
    }
}
