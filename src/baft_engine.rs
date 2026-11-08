//! BAFT main engine, config renderer (ADR-0019, design: docs/BAFT_ENGINE_INTEGRATION_DESIGN.md).
//!
//! BAFT runs as a separate Go process. This module only turns BAFT-TUNNEL desired state into
//! the JSON configuration that process reads. It is pure: it starts nothing, opens no socket
//! and reads no key. The output is derived, never edited by hand, and never read back.
//! Not wired into the engine selection yet (`EngineKind::Baft` arrives in a later phase).

use std::{
    fs,
    io::Write,
    net::{IpAddr, SocketAddr},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    config::{BaftSettings, RuntimeConfig, private_key_path},
    handshake::validate_identity,
    runtime_host::ChildSpec,
};

/// BAFT refuses this port for transport, routes and metrics.
const SSH_PORT: u16 = 22;
/// BAFT requires exactly one URI SAN of this form in node certificates and rejects any other
/// prefix at runtime (`internal/node/node.go:1325`), so certificates for the baft engine
/// carry `urn:baft:node:<id>`, not the `urn:baft-tunnel:node:<id>` used by the Rust engines.
const IDENTITY_PREFIX: &str = "urn:baft:node:";

/// Which side of the tunnel this node is (maps to BAFT carrier role).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Side {
    /// `serve`: accepts the carrier and forwards each stream to one fixed target.
    Serve {
        listen: SocketAddr,
        target: SocketAddr,
        /// TLS server name BAFT requires on listeners.
        server_name: String,
    },
    /// `forward`: dials the carrier and exposes a local listener.
    Forward {
        listen: SocketAddr,
        remote: SocketAddr,
        server_name: String,
        allow_non_loopback_listen: bool,
        /// Exact listener certificate (hex SHA-256 of the DER), enforced by the BAFT dialer.
        peer_pin: Option<String>,
    },
}

/// Runtime file locations. All must be absolute and live in the per-tunnel runtime directory.
#[derive(Debug, Clone)]
pub struct BaftPaths {
    pub ca: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
    /// Revocation list for this node. Listeners refuse revoked clients; dialers refuse a
    /// revoked listener (BAFT with dialer-side checks, see the pinned commit).
    pub revocation: Option<PathBuf>,
    pub unix_socket: PathBuf,
    pub metrics_listen: SocketAddr,
}

#[derive(Debug, Clone)]
pub struct BaftRenderInput {
    pub local_id: String,
    pub peer_id: String,
    pub tunnel_id: String,
    pub side: Side,
    pub max_streams: usize,
    pub paths: BaftPaths,
}

fn identity_uri(id: &str) -> String {
    format!("{IDENTITY_PREFIX}{id}")
}

fn absolute(path: &Path, field: &str) -> Result<String> {
    if !path.is_absolute() {
        bail!("{field} must be an absolute path");
    }
    match path.to_str() {
        Some(text) if !text.contains('\0') => Ok(text.to_owned()),
        _ => bail!("{field} must be valid UTF-8 without NUL"),
    }
}

fn not_ssh(addr: SocketAddr, field: &str) -> Result<()> {
    if addr.port() == SSH_PORT {
        bail!("{field}: port {SSH_PORT} is reserved for SSH management");
    }
    if addr.port() == 0 {
        bail!("{field}: port must be non-zero");
    }
    Ok(())
}

fn require_loopback(addr: SocketAddr, field: &str) -> Result<()> {
    if !addr.ip().is_loopback() {
        bail!("{field} must be a loopback address");
    }
    Ok(())
}

fn specific_unicast(addr: SocketAddr, field: &str) -> Result<()> {
    let ip: IpAddr = addr.ip();
    if ip.is_unspecified() || ip.is_multicast() {
        bail!("{field} must be a specific unicast address");
    }
    Ok(())
}

fn server_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 253
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-.".contains(&b))
    {
        bail!("server_name must be 1-253 ASCII letters, digits, '-' or '.'");
    }
    Ok(())
}

/// Render the BAFT node configuration as JSON. Keys are sorted, so output is deterministic.
pub fn render(input: &BaftRenderInput) -> Result<Value> {
    validate_identity(&input.local_id)?;
    validate_identity(&input.peer_id)?;
    validate_identity(&input.tunnel_id)?;
    if input.max_streams == 0 || input.max_streams > 1024 {
        bail!("max_streams must be between 1 and 1024");
    }
    let paths = &input.paths;
    let ca = absolute(&paths.ca, "ca")?;
    let cert = absolute(&paths.cert, "cert")?;
    let key = absolute(&paths.key, "key")?;
    let unix_socket = absolute(&paths.unix_socket, "unix_socket")?;
    require_loopback(paths.metrics_listen, "metrics_listen")?;
    not_ssh(paths.metrics_listen, "metrics_listen")?;

    let peer_uri = identity_uri(&input.peer_id);
    let max_flows = input.max_streams;

    let (role, transport_part, route) = match &input.side {
        Side::Serve {
            listen,
            target,
            server_name: name,
        } => {
            not_ssh(*listen, "listen")?;
            not_ssh(*target, "target")?;
            specific_unicast(*target, "target")?;
            server_name(name)?;
            (
                "listener",
                json!({"server": {
                    "listen": listen.to_string(),
                    "server_name": name,
                    "allowed_peer_identities": [peer_uri.clone()],
                }}),
                json!({
                    "id": input.tunnel_id,
                    "direction": "inbound",
                    "target": target.to_string(),
                    "allowed_peers": [peer_uri.clone()],
                    "traffic_class": "interactive",
                }),
            )
        }
        Side::Forward {
            listen,
            remote,
            server_name: name,
            allow_non_loopback_listen,
            peer_pin,
        } => {
            not_ssh(*listen, "listen")?;
            not_ssh(*remote, "remote")?;
            specific_unicast(*remote, "remote")?;
            server_name(name)?;
            if !*allow_non_loopback_listen {
                require_loopback(*listen, "listen")?;
            }
            let mut route = json!({
                "id": input.tunnel_id,
                "direction": "outbound",
                "listen": listen.to_string(),
                "remote_route": input.tunnel_id,
                "traffic_class": "interactive",
            });
            if *allow_non_loopback_listen && !listen.ip().is_loopback() {
                route["public"] = json!(true);
            }
            let mut peer = json!({
                "address": remote.to_string(),
                "server_name": name,
                "allowed_identity": peer_uri.clone(),
            });
            if let Some(pin) = peer_pin {
                if pin.len() != 64 || !pin.bytes().all(|b| b.is_ascii_hexdigit()) {
                    bail!("peer pin must be a 64-digit hexadecimal SHA-256");
                }
                peer["certificate_sha256"] = json!(pin.to_ascii_lowercase());
            }
            ("dialer", json!({"peer": peer}), route)
        }
    };

    let mut config = json!({
        "schema_version": 1,
        "node": {"id": input.local_id, "role": role},
        "tls": {
            "min_version": "1.3",
            "ca_file": ca,
            "cert_file": cert,
            "key_file": key,
            "session_tickets": false,
        },
        // Only the h2 mTLS carrier is exposed (ADR-0019): ws and uTLS wait for roadmap engines 4 and 5.
        "transport": {
            "primary": "h2",
            "h3_enabled": false,
            "shards": 1,
            "profile": "secure-fast",
        },
        "limits": {
            "max_flows": max_flows,
            "data_memory_mib": 64,
            "receive_initial_kib": 64,
            "receive_max_mib": 8,
            "replay_max_mib": 8,
        },
        "recovery": {"enabled": true, "retention_seconds": 30, "mode": "same_process"},
        "routes": [route],
        "management": {
            "unix_socket": unix_socket,
            "metrics_listen": paths.metrics_listen.to_string(),
        },
        // Telemetry stays off (no BCC); payload logging is never enabled.
        "logging": {"level": "info", "payload": false},
    });

    if let Some(part) = transport_part.as_object() {
        for (name, value) in part {
            config[name] = value.clone();
        }
    }
    if let Some(file) = &paths.revocation {
        config["revocation"] = json!({"file": absolute(file, "revocation")?});
    }
    Ok(config)
}

/// Everything `prepare_bundle` produced, all inside the per-tunnel runtime directory.
#[derive(Debug)]
pub struct Bundle {
    pub config_file: PathBuf,
}

/// Loopback metrics port for one node of a tunnel: stable per (tunnel id, node id), in 20000..30000.
/// A clash with another listener makes BAFT fail to start, which is visible; it is never silent.
fn metrics_port(tunnel_id: &str, local_id: &str) -> u16 {
    let digest = Sha256::digest(format!("{tunnel_id}/{local_id}").as_bytes());
    20000 + (u16::from_be_bytes([digest[0], digest[1]]) % 10000)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Build the render input from validated tunnel settings and copy the TLS material into
/// `runtime_dir` (must be an existing, empty, private directory on tmpfs). The key is read
/// from the configured file or systemd credential and written 0600; BAFT refuses keys
/// readable by group or other, which a systemd credential file may be.
pub fn prepare_bundle(cfg: &RuntimeConfig, runtime_dir: &Path, now_unix: i64) -> Result<Bundle> {
    let meta = fs::metadata(runtime_dir).context("runtime directory")?;
    if !meta.is_dir() || meta.permissions().mode() & 0o077 != 0 {
        bail!("runtime directory must be a directory with no group or other access");
    }
    if fs::read_dir(runtime_dir)?.next().is_some() {
        bail!("runtime directory must be empty; refusing to reuse a previous bundle");
    }
    let (local_id, peer_id, tunnel_id, max_streams, ca, crl, cert, key, key_credential, side) =
        match cfg {
            RuntimeConfig::Serve {
                listen,
                target,
                local_id,
                peer_id,
                tunnel_id,
                max_streams,
                ca,
                crl,
                cert,
                key,
                key_credential,
                ..
            } => (
                local_id,
                peer_id,
                tunnel_id,
                *max_streams,
                ca,
                crl,
                cert,
                key,
                key_credential,
                Side::Serve {
                    listen: listen.parse().context("listen")?,
                    target: target.parse().context("target")?,
                    server_name: format!("{local_id}.baft-tunnel.invalid"),
                },
            ),
            RuntimeConfig::Forward {
                listen,
                remote,
                server_name,
                local_id,
                peer_id,
                tunnel_id,
                max_streams,
                ca,
                crl,
                cert,
                key,
                key_credential,
                allow_non_loopback_listen,
                peer_fingerprint,
                ..
            } => (
                local_id,
                peer_id,
                tunnel_id,
                *max_streams,
                ca,
                crl,
                cert,
                key,
                key_credential,
                Side::Forward {
                    listen: listen.parse().context("listen")?,
                    remote: remote.parse().context("remote")?,
                    server_name: server_name.clone(),
                    allow_non_loopback_listen: *allow_non_loopback_listen,
                    peer_pin: (!peer_fingerprint.is_empty())
                        .then(|| crate::handshake::normalize_pin(peer_fingerprint)),
                },
            ),
        };
    let key_source = private_key_path(key.as_deref(), key_credential.as_deref())?;

    // Render the revocation list first: an expired or unreadable CRL stops the start.
    let revocation = revocation_from_crl(crl, now_unix)?;

    let paths = BaftPaths {
        ca: runtime_dir.join("ca.pem"),
        cert: runtime_dir.join("node.pem"),
        key: runtime_dir.join("node.key"),
        revocation: Some(runtime_dir.join("revocation.json")),
        unix_socket: runtime_dir.join("admin.sock"),
        metrics_listen: SocketAddr::from(([127, 0, 0, 1], metrics_port(tunnel_id, local_id))),
    };
    let input = BaftRenderInput {
        local_id: local_id.clone(),
        peer_id: peer_id.clone(),
        tunnel_id: tunnel_id.clone(),
        side,
        max_streams,
        paths: paths.clone(),
    };
    let config = render(&input)?;

    write_private(&paths.ca, &fs::read(ca).context("read ca")?)?;
    write_private(&paths.cert, &fs::read(cert).context("read cert")?)?;
    write_private(&paths.key, &fs::read(&key_source).context("read key")?)?;
    if let Some(path) = &paths.revocation {
        write_private(path, serde_json::to_string(&revocation)?.as_bytes())?;
    }
    let config_file = runtime_dir.join("baft.json");
    write_private(
        &config_file,
        serde_json::to_string_pretty(&config)?.as_bytes(),
    )?;
    Ok(Bundle { config_file })
}

/// Refuse a BAFT binary whose SHA-256 differs from the pinned value, or that others can write.
pub fn verify_binary(settings: &BaftSettings) -> Result<()> {
    crate::runtime_host::verify_artifact(&settings.binary, &settings.sha256)
}

/// The BAFT adapter's start recipe for the generic runtime host: `baft config validate`
/// first, `baft run` second, ready when the local listener accepts TCP.
pub fn child_spec(binary: &Path, config_file: &Path, listen: SocketAddr) -> ChildSpec {
    let file = config_file.display().to_string();
    let ready_ip = if listen.ip().is_unspecified() {
        std::net::Ipv4Addr::LOCALHOST.into()
    } else {
        listen.ip()
    };
    ChildSpec {
        binary: binary.to_path_buf(),
        validate_args: Some(vec![
            "config".into(),
            "validate".into(),
            "--file".into(),
            file.clone(),
        ]),
        run_args: vec!["run".into(), "--file".into(), file],
        ready_addr: SocketAddr::new(ready_ip, listen.port()),
        ready_timeout: Duration::from_secs(30),
        // Covers the dialer giving up after BAFT's recovery retention (risk R14).
        max_restarts: 5,
        restart_window: Duration::from_secs(300),
        grace: Duration::from_secs(10),
    }
}

/// `apply` entry for engine `baft`: verify the pinned binary, build the bundle, supervise.
pub async fn run(cfg: RuntimeConfig) -> Result<()> {
    let (settings, listen) = match &cfg {
        RuntimeConfig::Serve { baft, listen, .. } | RuntimeConfig::Forward { baft, listen, .. } => {
            (
                baft.clone()
                    .context("engine baft requires a baft section")?,
                listen
                    .parse::<SocketAddr>()
                    .context("listen must be an ip:port address")?,
            )
        }
    };
    verify_binary(&settings)?;
    let runtime_dir = std::env::var("RUNTIME_DIRECTORY")
        .ok()
        .and_then(|value| value.split(':').next().map(PathBuf::from))
        .context("RUNTIME_DIRECTORY is not set; start this engine from the systemd unit")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_secs() as i64;
    let bundle = prepare_bundle(&cfg, &runtime_dir, now)?;
    eprintln!("Starting the pinned BAFT engine; payload stays inside that process.");
    crate::runtime_host::supervise(
        &child_spec(&settings.binary, &bundle.config_file, listen),
        async {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("install SIGTERM handler");
            tokio::select! {
                _ = term.recv() => {},
                _ = tokio::signal::ctrl_c() => {},
            }
        },
        |phase| eprintln!("baft engine phase: {phase:?}"),
    )
    .await
}

/// BAFT accepts at most this many revocation entries (`maxRevocationEntries` upstream).
const MAX_REVOCATION_ENTRIES: usize = 4096;

/// Convert the node CRL (PEM, the same file the Rust engines verify) into BAFT's
/// `revocation.file` JSON. Fails closed: a CRL that is expired, has no `nextUpdate`, has
/// unparseable entries or exceeds BAFT's limit produces an error and no output, so the
/// caller keeps the previous bundle. BAFT revocation is add-only; removing an entry
/// needs an engine restart (docs, section 12).
/// BAFT's `identities` list only accepts its own `urn:baft:node:` prefix, so serials are used.
pub fn revocation_from_crl(crl_path: &Path, now_unix: i64) -> Result<Value> {
    use x509_parser::prelude::FromDer;
    let mut serials: Vec<String> = Vec::new();
    for der in crate::tls::revocation_lists(crl_path)? {
        let (_, crl) =
            x509_parser::revocation_list::CertificateRevocationList::from_der(der.as_ref())
                .map_err(|error| anyhow::anyhow!("parse CRL: {error}"))?;
        let next = crl
            .next_update()
            .ok_or_else(|| anyhow::anyhow!("CRL has no nextUpdate; refusing to use it"))?
            .timestamp();
        if next <= now_unix {
            bail!("CRL is expired; refusing to render a revocation list from it");
        }
        for revoked in crl.iter_revoked_certificates() {
            let bytes = revoked.user_certificate.to_bytes_be();
            if bytes.is_empty() || bytes.len() > 20 {
                bail!("CRL contains a serial outside 1..20 bytes");
            }
            let parts: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
            serials.push(parts.join(":"));
        }
    }
    serials.sort();
    serials.dedup();
    if serials.len() > MAX_REVOCATION_ENTRIES {
        bail!("CRL has more than {MAX_REVOCATION_ENTRIES} revoked serials; BAFT cannot load it");
    }
    Ok(json!({"serials": serials}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(revocation: bool) -> BaftPaths {
        BaftPaths {
            ca: "/run/baft-tunnel/t1/ca.pem".into(),
            cert: "/run/baft-tunnel/t1/node.pem".into(),
            key: "/run/baft-tunnel/t1/node.key".into(),
            revocation: revocation.then(|| "/run/baft-tunnel/t1/revocation.json".into()),
            unix_socket: "/run/baft-tunnel/t1/admin.sock".into(),
            metrics_listen: "127.0.0.1:9191".parse().unwrap(),
        }
    }

    fn serve() -> BaftRenderInput {
        BaftRenderInput {
            local_id: "abroad-1".into(),
            peer_id: "iran-1".into(),
            tunnel_id: "svc-main".into(),
            side: Side::Serve {
                listen: "198.51.100.10:8443".parse().unwrap(),
                target: "127.0.0.1:2080".parse().unwrap(),
                server_name: "abroad-1.baft-tunnel.invalid".into(),
            },
            max_streams: 8,
            paths: paths(true),
        }
    }

    fn forward() -> BaftRenderInput {
        BaftRenderInput {
            local_id: "iran-1".into(),
            peer_id: "abroad-1".into(),
            tunnel_id: "svc-main".into(),
            side: Side::Forward {
                listen: "127.0.0.1:1443".parse().unwrap(),
                remote: "198.51.100.10:8443".parse().unwrap(),
                server_name: "abroad-1.baft-tunnel.invalid".into(),
                allow_non_loopback_listen: false,
                peer_pin: None,
            },
            max_streams: 8,
            paths: paths(false),
        }
    }

    fn golden(name: &str) -> Value {
        let text = match name {
            "serve" => include_str!("testdata/baft/serve.json"),
            "forward" => include_str!("testdata/baft/forward.json"),
            "forward-hardened" => include_str!("testdata/baft/forward-hardened.json"),
            _ => unreachable!(),
        };
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn serve_matches_golden() {
        assert_eq!(render(&serve()).unwrap(), golden("serve"));
    }

    fn forward_hardened() -> BaftRenderInput {
        let mut input = forward();
        input.paths = paths(true);
        if let Side::Forward { peer_pin, .. } = &mut input.side {
            *peer_pin = Some("0123456789abcdef".repeat(4));
        }
        input
    }

    #[test]
    fn forward_with_revocation_and_pin_matches_golden() {
        if std::env::var_os("UPDATE_BAFT_GOLDEN").is_some() {
            std::fs::write(
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/src/testdata/baft/forward-hardened.json"
                ),
                serde_json::to_string_pretty(&render(&forward_hardened()).unwrap()).unwrap() + "\n",
            )
            .unwrap();
        }
        assert_eq!(
            render(&forward_hardened()).unwrap(),
            golden("forward-hardened")
        );
    }

    #[test]
    fn forward_matches_golden() {
        assert_eq!(render(&forward()).unwrap(), golden("forward"));
    }

    #[test]
    fn render_is_deterministic() {
        let a = serde_json::to_string(&render(&serve()).unwrap()).unwrap();
        let b = serde_json::to_string(&render(&serve()).unwrap()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn output_never_carries_key_material_or_payload_logging() {
        let text = serde_json::to_string(&render(&serve()).unwrap()).unwrap();
        assert!(!text.contains("BEGIN"));
        assert!(text.contains("\"payload\":false"));
        assert!(!text.contains("telemetry"));
    }

    #[test]
    fn rejects_ssh_port_everywhere() {
        let mut input = serve();
        input.side = Side::Serve {
            listen: "198.51.100.10:22".parse().unwrap(),
            target: "127.0.0.1:2080".parse().unwrap(),
            server_name: "a.invalid".into(),
        };
        assert!(render(&input).is_err());
        let mut input = serve();
        input.paths.metrics_listen = "127.0.0.1:22".parse().unwrap();
        assert!(render(&input).is_err());
        let mut input = forward();
        input.side = Side::Forward {
            listen: "127.0.0.1:22".parse().unwrap(),
            remote: "198.51.100.10:8443".parse().unwrap(),
            server_name: "a.invalid".into(),
            allow_non_loopback_listen: false,
            peer_pin: None,
        };
        assert!(render(&input).is_err());
    }

    #[test]
    fn rejects_non_loopback_metrics_and_forward_listen_without_opt_in() {
        let mut input = serve();
        input.paths.metrics_listen = "0.0.0.0:9191".parse().unwrap();
        assert!(render(&input).is_err());
        let mut input = forward();
        input.side = Side::Forward {
            listen: "0.0.0.0:1443".parse().unwrap(),
            remote: "198.51.100.10:8443".parse().unwrap(),
            server_name: "a.invalid".into(),
            allow_non_loopback_listen: false,
            peer_pin: None,
        };
        assert!(render(&input).is_err());
    }

    #[test]
    fn explicit_opt_in_marks_the_route_public() {
        let mut input = forward();
        input.side = Side::Forward {
            listen: "203.0.113.5:1443".parse().unwrap(),
            remote: "198.51.100.10:8443".parse().unwrap(),
            server_name: "a.invalid".into(),
            allow_non_loopback_listen: true,
            peer_pin: None,
        };
        let config = render(&input).unwrap();
        assert_eq!(config["routes"][0]["public"], json!(true));
    }

    #[test]
    fn rejects_bad_identities_relative_paths_and_unspecified_targets() {
        let mut input = serve();
        input.peer_id = "bad id".into();
        assert!(render(&input).is_err());
        let mut input = serve();
        input.paths.key = "node.key".into();
        assert!(render(&input).is_err());
        let mut input = serve();
        input.side = Side::Serve {
            listen: "198.51.100.10:8443".parse().unwrap(),
            target: "0.0.0.0:2080".parse().unwrap(),
            server_name: "a.invalid".into(),
        };
        assert!(render(&input).is_err());
        let mut input = serve();
        input.max_streams = 0;
        assert!(render(&input).is_err());
    }

    #[test]
    fn revocation_and_pin_are_rendered_for_dialers_too() {
        let pin = "AB".repeat(32);
        let mut input = forward();
        input.paths.revocation = Some("/run/baft-tunnel/t1/revocation.json".into());
        if let Side::Forward { peer_pin, .. } = &mut input.side {
            *peer_pin = Some(pin.clone());
        }
        let rendered = render(&input).unwrap();
        assert_eq!(
            rendered["revocation"]["file"],
            json!("/run/baft-tunnel/t1/revocation.json")
        );
        // BAFT matches the lowercase digest.
        assert_eq!(
            rendered["peer"]["certificate_sha256"],
            json!(pin.to_ascii_lowercase())
        );
        // No pin configured means no field, never an empty one.
        let mut input = forward();
        input.paths.revocation = None;
        let rendered = render(&input).unwrap();
        assert!(rendered["peer"].get("certificate_sha256").is_none());
        assert!(rendered.get("revocation").is_none());
        // A malformed pin stops the start.
        for bad in ["", "zz", &"a".repeat(63)] {
            let mut input = forward();
            if let Side::Forward { peer_pin, .. } = &mut input.side {
                *peer_pin = Some(bad.to_string());
            }
            assert!(render(&input).is_err(), "accepted pin {bad:?}");
        }
    }

    fn make_crl(dir: &Path, next_year: i32, serials: &[u64]) -> PathBuf {
        use rcgen::{
            CertificateParams, CertificateRevocationListParams, IsCa, KeyIdMethod, KeyPair,
            KeyUsagePurpose, RevokedCertParams, SerialNumber,
        };
        let now = rcgen::date_time_ymd(2026, 11, 8);
        let mut params = CertificateParams::new(Vec::new()).unwrap();
        params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let key = KeyPair::generate().unwrap();
        let issuer = rcgen::Issuer::new(params, key);
        let revoked_certs = serials
            .iter()
            .map(|serial| RevokedCertParams {
                serial_number: SerialNumber::from(*serial),
                revocation_time: now,
                reason_code: None,
                invalidity_date: None,
            })
            .collect();
        let crl = CertificateRevocationListParams {
            this_update: rcgen::date_time_ymd(2026, 11, 7),
            next_update: rcgen::date_time_ymd(next_year, 1, 1),
            crl_number: SerialNumber::from(1u64),
            issuing_distribution_point: None,
            revoked_certs,
            key_identifier_method: KeyIdMethod::Sha256,
        }
        .signed_by(&issuer)
        .unwrap();
        let path = dir.join("ca.crl");
        std::fs::write(&path, crl.pem().unwrap()).unwrap();
        path
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("baft-engine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn unix_now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    #[test]
    fn crl_serials_become_a_sorted_deduplicated_baft_list() {
        let dir = temp_dir("crl-ok");
        let path = make_crl(&dir, 2099, &[0x0a1b, 0x05, 0x05]);
        let list = revocation_from_crl(&path, unix_now()).unwrap();
        assert_eq!(list, json!({"serials": ["05", "0A:1B"]}));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_crl_gives_an_empty_list() {
        let dir = temp_dir("crl-empty");
        let path = make_crl(&dir, 2099, &[]);
        assert_eq!(
            revocation_from_crl(&path, unix_now()).unwrap(),
            json!({"serials": []})
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn expired_or_missing_crl_fails_closed() {
        let dir = temp_dir("crl-bad");
        let path = make_crl(&dir, 2099, &[7]);
        assert!(revocation_from_crl(&path, 4_102_444_800).is_err());
        assert!(revocation_from_crl(&dir.join("absent.crl"), unix_now()).is_err());
        std::fs::write(dir.join("junk.crl"), "not a crl").unwrap();
        assert!(revocation_from_crl(&dir.join("junk.crl"), unix_now()).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn private_dir(name: &str) -> PathBuf {
        let dir = temp_dir(name);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    fn serve_config(src: &Path, crl: &Path) -> RuntimeConfig {
        serde_json::from_value(json!({
            "mode": "serve", "listen": "198.51.100.10:8443", "target": "127.0.0.1:2080",
            "local_id": "abroad-1", "peer_id": "iran-1", "tunnel_id": "svc-main",
            "direction": "direct", "engine": "baft",
            "ca": write_file(src, "ca.crt", b"CA"), "crl": crl,
            "cert": write_file(src, "node.crt", b"CERT"),
            "key": write_file(src, "node.key", b"KEY-BYTES"),
            "baft": {"binary": "/bin/true", "sha256": "0".repeat(64)}
        }))
        .unwrap()
    }

    #[test]
    fn bundle_has_private_files_and_a_listener_revocation_list() {
        let src = temp_dir("bundle-src");
        let crl = make_crl(&src, 2099, &[0x0a1b]);
        let cfg = serve_config(&src, &crl);
        let out = private_dir("bundle-out");
        let bundle = prepare_bundle(&cfg, &out, unix_now()).unwrap();
        for name in [
            "ca.pem",
            "node.pem",
            "node.key",
            "revocation.json",
            "baft.json",
        ] {
            let mode = std::fs::metadata(out.join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{name}");
        }
        assert_eq!(std::fs::read(out.join("node.key")).unwrap(), b"KEY-BYTES");
        let rendered: Value =
            serde_json::from_slice(&std::fs::read(&bundle.config_file).unwrap()).unwrap();
        assert_eq!(rendered["node"]["role"], json!("listener"));
        assert_eq!(
            rendered["revocation"]["file"],
            json!(out.join("revocation.json"))
        );
        let revoked: Value =
            serde_json::from_slice(&std::fs::read(out.join("revocation.json")).unwrap()).unwrap();
        assert_eq!(revoked, json!({"serials": ["0A:1B"]}));
        // A used directory is refused, never reused.
        assert!(prepare_bundle(&cfg, &out, unix_now()).is_err());
        std::fs::remove_dir_all(src).unwrap();
        std::fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn expired_crl_stops_the_bundle_before_any_file_is_written() {
        let src = temp_dir("bundle-exp-src");
        let crl = make_crl(&src, 2099, &[1]);
        let cfg = serve_config(&src, &crl);
        let out = private_dir("bundle-exp-out");
        assert!(prepare_bundle(&cfg, &out, 4_102_444_800).is_err());
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
        std::fs::remove_dir_all(src).unwrap();
        std::fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn group_accessible_runtime_directory_is_refused() {
        let src = temp_dir("bundle-perm-src");
        let crl = make_crl(&src, 2099, &[]);
        let cfg = serve_config(&src, &crl);
        let out = temp_dir("bundle-perm-out");
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o750)).unwrap();
        assert!(prepare_bundle(&cfg, &out, unix_now()).is_err());
        std::fs::remove_dir_all(src).unwrap();
        std::fs::remove_dir_all(out).unwrap();
    }

    fn script(dir: &Path, body: &str, mode: u32) -> PathBuf {
        let path = write_file(dir, "baft", format!("#!/bin/sh\n{body}\n").as_bytes());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn binary_hash_and_mode_are_enforced() {
        let dir = temp_dir("binary");
        let path = script(&dir, "exit 0", 0o755);
        let good = hex::encode(Sha256::digest(std::fs::read(&path).unwrap()));
        let settings = BaftSettings {
            binary: path.clone(),
            sha256: good.clone(),
        };
        verify_binary(&settings).unwrap();
        let wrong = BaftSettings {
            binary: path.clone(),
            sha256: "0".repeat(64),
        };
        assert!(verify_binary(&wrong).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(verify_binary(&settings).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn engine_baft_config_rejects_pins_missing_section_and_bad_hash() {
        let base = |extra: Value| -> RuntimeConfig {
            let mut value = json!({
                "mode": "serve", "listen": "198.51.100.10:8443", "target": "127.0.0.1:2080",
                "local_id": "abroad-1", "peer_id": "iran-1", "tunnel_id": "svc-main",
                "direction": "direct", "engine": "baft",
                "ca": "/x/ca.crt", "crl": "/x/ca.crl", "cert": "/x/node.crt", "key": "/x/node.key"
            });
            for (name, v) in extra.as_object().unwrap() {
                value[name] = v.clone();
            }
            serde_json::from_value(value).unwrap()
        };
        let good_baft =
            json!({"binary": "/usr/local/lib/baft-tunnel/baft", "sha256": "a".repeat(64)});
        // missing baft section
        assert!(base(json!({})).validate().is_err());
        // pin is not used by this engine
        assert!(
            base(json!({"baft": good_baft, "peer_fingerprint": "b".repeat(64)}))
                .validate()
                .is_err()
        );
        // bad hash and relative binary
        assert!(base(json!({"baft": {"sha256": "zz"}})).validate().is_err());
        assert!(
            base(json!({"baft": {"binary": "baft", "sha256": "a".repeat(64)}}))
                .validate()
                .is_err()
        );
        // a dialer may pin exactly one listener certificate; a listener cannot pin clients
        let forward = |fingerprint: &str| -> RuntimeConfig {
            serde_json::from_value(json!({
                "mode": "forward", "listen": "127.0.0.1:1443", "remote": "198.51.100.10:8443",
                "server_name": "abroad-1.baft-tunnel.invalid", "peer_fingerprint": fingerprint,
                "local_id": "iran-1", "peer_id": "abroad-1", "tunnel_id": "svc-main",
                "direction": "direct", "engine": "baft",
                "ca": "/x/ca.crt", "crl": "/x/ca.crl", "cert": "/x/node.crt", "key": "/x/node.key",
                "baft": good_baft
            }))
            .unwrap()
        };
        // These paths do not exist, so validation stops at the key/TLS material check; the
        // pin rules run before it and are what these assertions pin down.
        let message = |cfg: RuntimeConfig| format!("{:#}", cfg.validate().unwrap_err());
        assert!(!message(forward(&"ab".repeat(32))).contains("pin"));
        assert!(
            message(forward(&format!("{},{}", "ab".repeat(32), "cd".repeat(32))))
                .contains("exactly one")
        );
        assert!(message(forward("zz")).contains("SHA-256"));
        assert!(
            message(base(
                json!({"baft": good_baft, "peer_fingerprint": "ab".repeat(32)})
            ))
            .contains("listener")
        );
        // unknown field inside baft is refused
        assert!(
            serde_json::from_value::<RuntimeConfig>(json!({
                "mode": "serve", "listen": "198.51.100.10:8443", "target": "127.0.0.1:2080",
                "local_id": "a", "peer_id": "b", "tunnel_id": "t", "direction": "direct",
                "engine": "baft", "baft": {"sha256": "a".repeat(64), "extra": 1}
            }))
            .is_err()
        );
    }
}
