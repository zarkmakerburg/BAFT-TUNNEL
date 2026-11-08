//! BAFT main engine, config renderer (ADR-0019, design: docs/BAFT_ENGINE_INTEGRATION_DESIGN.md).
//!
//! BAFT runs as a separate Go process. This module only turns BAFT-TUNNEL desired state into
//! the JSON configuration that process reads. It is pure: it starts nothing, opens no socket
//! and reads no key. The output is derived, never edited by hand, and never read back.
//! Not wired into the engine selection yet (`EngineKind::Baft` arrives in a later phase).

use std::{
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::handshake::validate_identity;

/// BAFT refuses this port for transport, routes and metrics.
const SSH_PORT: u16 = 22;
const IDENTITY_PREFIX: &str = "urn:baft-tunnel:node:";

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
    },
}

/// Runtime file locations. All must be absolute and live in the per-tunnel runtime directory.
#[derive(Debug, Clone)]
pub struct BaftPaths {
    pub ca: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
    /// Listener only. Absent on dialers (BAFT enforces revocation on listeners only).
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
            (
                "dialer",
                json!({"peer": {
                    "address": remote.to_string(),
                    "server_name": name,
                    "allowed_identity": peer_uri.clone(),
                }}),
                route,
            )
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
    match (&paths.revocation, role) {
        (Some(file), "listener") => {
            config["revocation"] = json!({"file": absolute(file, "revocation")?});
        }
        (Some(_), _) => bail!("revocation is only enforced by BAFT listeners"),
        (None, _) => {}
    }
    Ok(config)
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
            },
            max_streams: 8,
            paths: paths(false),
        }
    }

    fn golden(name: &str) -> Value {
        let text = match name {
            "serve" => include_str!("testdata/baft/serve.json"),
            "forward" => include_str!("testdata/baft/forward.json"),
            _ => unreachable!(),
        };
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn serve_matches_golden() {
        assert_eq!(render(&serve()).unwrap(), golden("serve"));
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
    fn revocation_only_on_listeners() {
        let mut input = forward();
        input.paths.revocation = Some("/run/baft-tunnel/t1/revocation.json".into());
        assert!(render(&input).is_err());
        let mut input = serve();
        input.paths.revocation = None;
        assert!(render(&input).unwrap().get("revocation").is_none());
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
}
