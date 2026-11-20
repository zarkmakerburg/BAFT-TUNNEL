//! Fleet start-up (ADR-0045 F1, read-only, Preview): reads the optional `--fleet-config` file and
//! starts the hub listener and/or the member reporter inside the agent. Off unless the flag is
//! given. The file holds only certificate file paths and addresses, never key material. Nothing
//! here can start, stop or change a tunnel: the hub only stores reports, the member only sends one.
#![allow(dead_code)]

use crate::agent::Ctx;
use crate::{fleet_report::ReporterConfig, key_custody};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
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
    /// Legacy/preview file source. New Hub identities use the systemd credential source below.
    #[serde(default)]
    pub key: Option<PathBuf>,
    #[serde(default)]
    pub key_credential: Option<String>,
    /// Optional pre-enrollment Join listener, separate from the mandatory-mTLS report socket.
    #[serde(default)]
    pub join: Option<JoinSection>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinSection {
    pub listen: SocketAddr,
    pub advertise_host: String,
    pub advertise_port: u16,
    pub hub_id: String,
    pub hub_name: String,
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

fn validate_key_source(key: Option<&Path>, credential: Option<&str>) -> Result<()> {
    ensure!(
        key.is_some() ^ credential.is_some(),
        "Hub key must use exactly one source: key or key_credential"
    );
    if let Some(path) = key {
        check_file("hub key", path)?;
    }
    if let Some(identifier) = credential {
        key_custody::validate_systemd_credential_id(identifier)?;
    }
    Ok(())
}

fn resolve_key_source(key: Option<&Path>, credential: Option<&str>) -> Result<PathBuf> {
    validate_key_source(key, credential)?;
    match (key, credential) {
        (Some(path), None) => Ok(path.to_path_buf()),
        (None, Some(identifier)) => key_custody::systemd_credential_path(identifier),
        _ => unreachable!("validated key source has exactly one variant"),
    }
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

    /// Load the root-managed per-instance config when present. A missing file means this Agent
    /// has no Fleet role; any other error or non-regular file fails closed. This avoids replacing
    /// a user's managed systemd ExecStart merely to attach the optional Fleet Hub role.
    pub fn load_if_present(path: &Path) -> Result<Option<Self>> {
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error)
                .with_context(|| format!("cannot inspect Fleet config {}", path.display())),
            Ok(meta) => {
                ensure!(
                    meta.file_type().is_file() && meta.len() <= MAX_CONFIG_BYTES,
                    "fleet config must be a regular file of at most {MAX_CONFIG_BYTES} bytes"
                );
                Self::load(path).map(Some)
            }
        }
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
            ] {
                check_file(l, p)?;
            }
            validate_key_source(h.key.as_deref(), h.key_credential.as_deref())?;
            if let Some(join) = &h.join {
                ensure!(
                    join.listen.port() != 0,
                    "Fleet Join listen port must not be zero"
                );
                ensure!(
                    join.advertise_port != 0 && join.advertise_port == join.listen.port(),
                    "Fleet Join advertised port must equal its nonzero bind port"
                );
                crate::tls::server_name(&join.advertise_host)
                    .context("Fleet Join advertised host is not a valid TLS name")?;
                ensure!(
                    crate::fleet_ca::valid_node_id(&join.hub_id),
                    "Fleet Join hub_id is invalid"
                );
                crate::tls::server_name(&join.hub_name)
                    .context("Fleet Join hub_name is not a valid TLS name")?;
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

/// Validate a Join request against the live Enroller offer. The listener never signs: it only
/// returns already-issued public material after operator confirmation.
pub(crate) fn admit_join_request(
    enroller: &mut crate::fleet_enroll::Enroller,
    request: crate::fleet_join::JoinRequest,
    now: u64,
    ca: &Path,
    crl: &Path,
    hub_cert: &Path,
) -> crate::fleet_join::Admission {
    use crate::fleet_enroll::State;
    let offer_id = request.offer_id.clone();
    let submit = enroller.submit(
        &request.offer_id,
        &request.node_id,
        &request.token,
        &request.nonce,
        &request.csr_pem,
        now,
    );
    if submit.is_err() {
        return match enroller.state_and_expiry(&offer_id) {
            Ok((State::Closed, _)) | Err(crate::fleet_enroll::Error::Unknown) => {
                crate::fleet_join::Admission::Close
            }
            _ => crate::fleet_join::Admission::Retry,
        };
    }
    let (state, expires) = match enroller.state_and_expiry(&offer_id) {
        Ok(value) => value,
        Err(_) => return crate::fleet_join::Admission::Close,
    };
    if expires < now || state == State::Closed {
        return crate::fleet_join::Admission::Close;
    }
    if state != State::Issued {
        return crate::fleet_join::Admission::Retry;
    }
    let certificate = match enroller.certificate_for_delivery(&offer_id) {
        Ok(value) => value.to_owned(),
        Err(_) => return crate::fleet_join::Admission::Close,
    };
    let read_public = |path: &Path| std::fs::read_to_string(path).ok().filter(|v| !v.is_empty());
    let (Some(ca_certificate_pem), Some(crl_pem), Some(hub_certificate_pem)) =
        (read_public(ca), read_public(crl), read_public(hub_cert))
    else {
        return crate::fleet_join::Admission::Close;
    };
    crate::fleet_join::Admission::Deliver {
        response: crate::fleet_join::JoinResponse {
            ok: true,
            member_certificate_pem: Some(certificate),
            ca_certificate_pem: Some(ca_certificate_pem),
            crl_pem: Some(crl_pem),
            hub_certificate_pem: Some(hub_certificate_pem),
            reason: None,
        },
        expires,
    }
}

/// Apply the transport ACK to the exact offer. A late ACK after Deny or expiry is a no-op.
pub(crate) fn acknowledge_join_delivery(
    enroller: &mut crate::fleet_enroll::Enroller,
    offer_id: &str,
    now: u64,
) {
    let _ = enroller.mark_delivered(offer_id, now);
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Start what the file asks for. The member needs the agent's `--instance` (its report names it).
/// Failures to bind or load certificates are returned so the unit fails visibly instead of
/// running without the Fleet role the operator configured.
pub async fn start(ctx: Arc<Ctx>, cfg: FleetConfig, instance: Option<&str>) -> Result<()> {
    // Validate and prepare every configured role before spawning any task. A bad Join bind,
    // certificate, or member identity must not leave an already-running Hub behind.
    let FleetConfig { hub, member } = cfg;
    let member_config = if let Some(m) = member {
        let instance = instance.context("a Fleet member needs the agent's --instance")?;
        crate::tls::client_config(&m.ca, &m.crl, &m.cert, &m.key)
            .context("validate Fleet member TLS material")?;
        Some(ReporterConfig {
            hub_addr: m.hub_addr,
            hub_name: m.hub_name,
            hub_id: m.hub_id,
            ca: m.ca,
            crl: m.crl,
            cert: m.cert,
            key: m.key,
            instance: instance.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        })
    } else {
        None
    };

    // The tasks live as long as the agent process; shutdown is the process exit.
    let (_stop, rx) = watch::channel(false);
    std::mem::forget(_stop);

    if let Some(h) = hub {
        let hub_key = resolve_key_source(h.key.as_deref(), h.key_credential.as_deref())?;
        let hub_acceptor = Arc::new(crate::tls::ReloadingAcceptor::new(
            &h.ca, &h.crl, &h.cert, &hub_key,
        )?);
        let hub_listener = TcpListener::bind(&h.listen)
            .await
            .with_context(|| format!("cannot bind fleet hub on {}", h.listen))?;
        let hub_bound = hub_listener.local_addr()?;

        // Prepare Join completely before starting either listener.
        let join_prepared = if let Some(join) = h.join {
            let listener = TcpListener::bind(join.listen)
                .await
                .with_context(|| format!("cannot bind Fleet Join listener on {}", join.listen))?;
            let bound = listener.local_addr()?;
            ensure!(
                bound.port() == join.advertise_port,
                "Fleet Join listener bound port differs from advertised port"
            );
            let acceptor = crate::tls::fleet_join_acceptor(&h.cert, &hub_key)?;
            Some((
                listener,
                acceptor,
                bound,
                h.ca.clone(),
                h.crl.clone(),
                h.cert.clone(),
            ))
        } else {
            None
        };

        // Publish the bound address only after all startup validation and binds have succeeded.
        *ctx.fleet_hub_listen
            .lock()
            .map_err(|_| anyhow::anyhow!("Fleet Hub listener state is poisoned"))? =
            Some(hub_bound);

        eprintln!(
            "Fleet hub listening on {} (mutual TLS, read-only)",
            h.listen
        );
        tokio::spawn(crate::fleet_hub::serve(
            hub_listener,
            hub_acceptor,
            ctx.clone(),
            rx.clone(),
        ));

        if let Some((join_listener, join_acceptor, join_bound, ca, crl, hub_cert)) = join_prepared {
            let admission_ctx = ctx.clone();
            let admit = move |request, _peer| {
                let context = admission_ctx.clone();
                let ca = ca.clone();
                let crl = crl.clone();
                let hub_cert = hub_cert.clone();
                async move {
                    let mut enroller = context
                        .fleet_enroll
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner());
                    admit_join_request(&mut enroller, request, unix_now(), &ca, &crl, &hub_cert)
                }
            };
            let ack_ctx = ctx.clone();
            let acknowledged = move |offer_id: String, _node_id: String| {
                let context = ack_ctx.clone();
                async move {
                    let mut enroller = context
                        .fleet_enroll
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner());
                    acknowledge_join_delivery(&mut enroller, &offer_id, unix_now());
                }
            };
            eprintln!(
                "Fleet Join listener listening on {} (TLS 1.3, pinned Hub identity)",
                join_bound
            );
            tokio::spawn(crate::fleet_join::serve_shared(
                join_listener,
                join_acceptor,
                admit,
                acknowledged,
                rx.clone(),
            ));
        }
    }

    if let Some(rc) = member_config {
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
    fn join_ack_advances_only_the_issued_offer_to_awaiting_report() {
        let mut enroller = crate::fleet_enroll::Enroller::default();
        let id = crate::fleet_enroll::test_support::issued_offer(&mut enroller, "edge-ack", 100);
        acknowledge_join_delivery(&mut enroller, &id, 101);
        assert_eq!(
            enroller.state_and_expiry(&id).unwrap().0,
            crate::fleet_enroll::State::AwaitingReport
        );
    }

    #[test]
    fn join_ack_racing_deny_cannot_resurrect_offer() {
        let mut enroller = crate::fleet_enroll::Enroller::default();
        let id = crate::fleet_enroll::test_support::issued_offer(&mut enroller, "edge-deny", 100);
        enroller.deny(&id).unwrap();
        acknowledge_join_delivery(&mut enroller, &id, 101);
        assert_eq!(
            enroller.state_and_expiry(&id).unwrap().0,
            crate::fleet_enroll::State::Closed
        );
    }

    #[test]
    fn join_ack_after_expiry_closes_without_delivery_transition() {
        let mut enroller = crate::fleet_enroll::Enroller::default();
        let id = crate::fleet_enroll::test_support::issued_offer(&mut enroller, "edge-expire", 100);
        let expires = enroller.state_and_expiry(&id).unwrap().1;
        acknowledge_join_delivery(&mut enroller, &id, expires + 1);
        assert_eq!(
            enroller.state_and_expiry(&id).unwrap().0,
            crate::fleet_enroll::State::Closed
        );
    }

    #[test]
    fn per_instance_config_is_optional_but_symlinks_fail_closed() {
        let d = tmp("optional");
        let path = d.join("fleet.json");
        assert!(FleetConfig::load_if_present(&path).unwrap().is_none());
        let target = d.join("target.json");
        std::fs::write(&target, "{}\n").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(FleetConfig::load_if_present(&path).is_err());
        let _ = std::fs::remove_dir_all(d);
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
        let hub_credential = format!(
            r#"{{"hub":{{"listen":"127.0.0.1:18443","ca":"{ca}","crl":"{crl}","cert":"{cert}","key_credential":"fleet-hub-key"}}}}"#
        );
        assert!(FleetConfig::parse(&hub_credential).unwrap().hub.is_some());
        let both_key_sources = hub_credential.replace(
            "\"key_credential\":\"fleet-hub-key\"",
            &format!("\"key\":\"{key}\",\"key_credential\":\"fleet-hub-key\""),
        );
        assert!(FleetConfig::parse(&both_key_sources).is_err());
        let no_key_source = hub.replace(&format!(",\"key\":\"{key}\""), "");
        assert!(FleetConfig::parse(&no_key_source).is_err());
        let invalid_credential = hub_credential.replace("fleet-hub-key", "../hub-key");
        assert!(FleetConfig::parse(&invalid_credential).is_err());
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

    #[test]
    fn fleet_config_validates_optional_join_section() {
        let d = tmp("join-config");
        let [ca, crl, cert, key] = files(&d);
        let valid = format!(
            r#"{{"hub":{{"listen":"127.0.0.1:18443","ca":"{ca}","crl":"{crl}","cert":"{cert}","key":"{key}","join":{{"listen":"0.0.0.0:18444","advertise_host":"hub.example.test","advertise_port":18444,"hub_id":"hub-1","hub_name":"hub.example.test"}}}}}}"#
        );
        assert!(
            FleetConfig::parse(&valid).is_ok(),
            "valid optional Join config must parse"
        );
        let zero_port = valid.replace("18444", "0");
        assert!(
            FleetConfig::parse(&zero_port).is_err(),
            "zero Join port must be rejected"
        );
        let bad_host = valid.replace("hub.example.test", "not a host");
        assert!(
            FleetConfig::parse(&bad_host).is_err(),
            "invalid Join TLS host must be rejected"
        );
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
