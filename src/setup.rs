//! Guided setup, slice S1: build and validate a planned configuration without touching the host.
//! Nothing here reads or writes files, opens sockets or generates keys; later slices add that.

use std::{net::IpAddr, path::PathBuf};

use anyhow::{Result, bail};
use clap::ValueEnum;
use serde_json::{Value, json};

use crate::{authorization::ConnectionDirection, config::EngineKind, handshake::validate_identity};

/// Placeholder written where pairing has not provided the peer's trust material yet.
pub const PEER_KEY_PLACEHOLDER: &str = "SET_BY_PAIRING";

pub const DEFAULT_TUNNEL_PORT: u16 = 8443;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum Role {
    /// Hosts the real service and listens for the tunnel.
    Serve,
    /// Listens locally for users and dials the serving peer.
    Forward,
}

#[derive(Clone, Debug)]
pub struct PlanRequest {
    pub engine: EngineKind,
    pub role: Role,
    pub direction: ConnectionDirection,
    pub node_id: String,
    pub peer_id: String,
    pub tunnel_id: String,
    /// The one port the operator chooses: the service that is exposed through the tunnel.
    pub service_port: u16,
    pub tunnel_port: u16,
    /// Required for `forward` (an IP address); rejected for `serve`.
    pub peer_host: Option<String>,
    /// Required for `tcp-tls` + `forward`; rejected otherwise.
    pub server_name: Option<String>,
    pub dir: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PlannedFile {
    pub path: PathBuf,
    pub mode: &'static str,
    pub note: &'static str,
}

#[derive(Debug)]
pub struct SetupPlan {
    pub config: Value,
    pub files: Vec<PlannedFile>,
    pub pending: Vec<&'static str>,
}

fn check_port(value: u16, what: &str) -> Result<()> {
    if value < 1024 {
        bail!("{what} must be 1024-65535 (privileged ports are not used by setup)");
    }
    Ok(())
}

pub fn plan(req: &PlanRequest) -> Result<SetupPlan> {
    validate_identity(&req.node_id)?;
    validate_identity(&req.peer_id)?;
    validate_identity(&req.tunnel_id)?;
    if req.node_id == req.peer_id {
        bail!("node and peer identities must differ");
    }
    check_port(req.service_port, "service port")?;
    check_port(req.tunnel_port, "tunnel port")?;
    if req.service_port == req.tunnel_port {
        bail!("service port and tunnel port must differ");
    }
    if !req.dir.is_absolute() {
        bail!("setup directory must be an absolute path");
    }
    let peer_ip = match (req.role, req.peer_host.as_deref()) {
        (Role::Forward, Some(host)) => Some(
            host.parse::<IpAddr>()
                .map_err(|_| anyhow::anyhow!("peer host must be an IP address"))?,
        ),
        (Role::Forward, None) => bail!("role forward requires a peer host"),
        (Role::Serve, Some(_)) => bail!("role serve must not set a peer host"),
        (Role::Serve, None) => None,
    };
    match (req.engine, req.role, req.server_name.as_deref()) {
        (EngineKind::TcpTls, Role::Forward, Some(name)) => validate_identity(name)?,
        (EngineKind::TcpTls, Role::Forward, None) => {
            bail!("tcp-tls with role forward requires a server name")
        }
        (_, _, Some(_)) => bail!("server name is only used by tcp-tls with role forward"),
        _ => {}
    }

    let path = |name: &str| req.dir.join(name);
    let text = |name: &str| path(name).to_string_lossy().into_owned();
    let mut config = json!({
        "engine": match req.engine { EngineKind::TcpTls => "tcp-tls", EngineKind::NoiseTcp => "noise-tcp" },
        "direction": req.direction.as_str(),
        "local_id": req.node_id,
        "peer_id": req.peer_id,
        "tunnel_id": req.tunnel_id,
        "max_streams": 8,
    });
    let map = config.as_object_mut().expect("object literal");
    match req.role {
        Role::Serve => {
            map.insert("mode".into(), json!("serve"));
            map.insert(
                "listen".into(),
                json!(format!("0.0.0.0:{}", req.tunnel_port)),
            );
            map.insert(
                "target".into(),
                json!(format!("127.0.0.1:{}", req.service_port)),
            );
        }
        Role::Forward => {
            map.insert("mode".into(), json!("forward"));
            map.insert(
                "listen".into(),
                json!(format!("127.0.0.1:{}", req.service_port)),
            );
            let ip = peer_ip.expect("checked above");
            let remote = std::net::SocketAddr::new(ip, req.tunnel_port).to_string();
            map.insert("remote".into(), json!(remote));
            if let Some(name) = &req.server_name {
                map.insert("server_name".into(), json!(name));
            }
        }
    }

    let mut files = vec![PlannedFile {
        path: path("config.json"),
        mode: "0640",
        note: "tunnel configuration (no secrets)",
    }];
    let mut pending = Vec::new();
    match req.engine {
        EngineKind::NoiseTcp => {
            map.insert("key".into(), json!(text("node.noise")));
            map.insert("noise_peer_public".into(), json!(PEER_KEY_PLACEHOLDER));
            files.push(PlannedFile {
                path: path("node.noise"),
                mode: "0600",
                note: "node private key, generated locally and never exported",
            });
            pending.push("peer Noise public key (from the pairing bundle)");
        }
        EngineKind::TcpTls => {
            map.insert("ca".into(), json!(text("ca.crt")));
            map.insert("crl".into(), json!(text("ca.crl")));
            map.insert("cert".into(), json!(text("node.crt")));
            map.insert("key".into(), json!(text("node.key")));
            map.insert("peer_fingerprint".into(), json!(PEER_KEY_PLACEHOLDER));
            for (name, mode, note) in [
                (
                    "ca.crt",
                    "0644",
                    "trusted CA certificate(s) from the offline CA",
                ),
                ("ca.crl", "0644", "current CRL(s), one per CA"),
                ("node.crt", "0644", "this node's certificate"),
                ("node.key", "0600", "this node's private key"),
            ] {
                files.push(PlannedFile {
                    path: path(name),
                    mode,
                    note,
                });
            }
            pending.push("peer certificate SHA-256 pin (one, or two during rollover)");
        }
    }
    Ok(SetupPlan {
        config,
        files,
        pending,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RuntimeConfig;

    fn request(engine: EngineKind, role: Role) -> PlanRequest {
        PlanRequest {
            engine,
            role,
            direction: ConnectionDirection::Direct,
            node_id: "iran".into(),
            peer_id: "abroad".into(),
            tunnel_id: "demo".into(),
            service_port: 8080,
            tunnel_port: DEFAULT_TUNNEL_PORT,
            peer_host: (role == Role::Forward).then(|| "203.0.113.7".to_owned()),
            server_name: (engine == EngineKind::TcpTls && role == Role::Forward)
                .then(|| "abroad.lab".to_owned()),
            dir: PathBuf::from("/etc/baft-tunnel"),
        }
    }

    /// The template must deserialize into the real runtime config once the placeholder is
    /// replaced; this fails if setup output drifts from the config schema.
    fn parses_as_runtime_config(plan: &SetupPlan) -> RuntimeConfig {
        let text = plan
            .config
            .to_string()
            .replace(PEER_KEY_PLACEHOLDER, &"ab".repeat(32));
        serde_json::from_str(&text).expect("planned config matches the runtime schema")
    }

    #[test]
    fn all_engine_and_role_combinations_match_the_runtime_schema() {
        for engine in [EngineKind::NoiseTcp, EngineKind::TcpTls] {
            for role in [Role::Serve, Role::Forward] {
                let plan = plan(&request(engine, role)).unwrap();
                parses_as_runtime_config(&plan);
                assert!(!plan.pending.is_empty());
            }
        }
    }

    #[test]
    fn serve_and_forward_use_one_service_port_and_a_fixed_loopback_boundary() {
        let serve = plan(&request(EngineKind::NoiseTcp, Role::Serve)).unwrap();
        assert_eq!(serve.config["listen"], "0.0.0.0:8443");
        assert_eq!(serve.config["target"], "127.0.0.1:8080");
        let forward = plan(&request(EngineKind::NoiseTcp, Role::Forward)).unwrap();
        assert_eq!(forward.config["listen"], "127.0.0.1:8080");
        assert_eq!(forward.config["remote"], "203.0.113.7:8443");
    }

    #[test]
    fn plan_is_deterministic_and_lists_private_keys_as_0600() {
        let a = plan(&request(EngineKind::TcpTls, Role::Forward)).unwrap();
        let b = plan(&request(EngineKind::TcpTls, Role::Forward)).unwrap();
        assert_eq!(a.config, b.config);
        assert_eq!(a.files, b.files);
        for file in &a.files {
            let secret = file.path.ends_with("node.key") || file.path.ends_with("node.noise");
            assert_eq!(file.mode == "0600", secret, "{:?}", file.path);
        }
    }

    #[test]
    fn invalid_requests_are_rejected() {
        let ok = request(EngineKind::NoiseTcp, Role::Forward);
        let bad = |edit: &dyn Fn(&mut PlanRequest)| {
            let mut req = ok.clone();
            edit(&mut req);
            plan(&req).is_err()
        };
        assert!(bad(&|r| r.service_port = 80));
        assert!(bad(&|r| r.tunnel_port = 443));
        assert!(bad(&|r| r.tunnel_port = r.service_port));
        assert!(bad(&|r| r.peer_id = r.node_id.clone()));
        assert!(bad(&|r| r.tunnel_id = "bad id".into()));
        assert!(bad(&|r| r.node_id = String::new()));
        assert!(bad(&|r| r.peer_host = None));
        assert!(bad(&|r| r.peer_host = Some("example.com".into())));
        assert!(bad(&|r| r.peer_host = Some("1.2.3.4;id".into())));
        assert!(bad(&|r| r.dir = PathBuf::from("relative/dir")));
        assert!(bad(&|r| r.server_name = Some("abroad.lab".into())));
        let mut serve = request(EngineKind::NoiseTcp, Role::Serve);
        serve.peer_host = Some("203.0.113.7".into());
        assert!(plan(&serve).is_err());
        let mut tls = request(EngineKind::TcpTls, Role::Forward);
        tls.server_name = None;
        assert!(plan(&tls).is_err());
    }

    #[test]
    fn plan_contains_no_private_key_material() {
        let plan = plan(&request(EngineKind::NoiseTcp, Role::Serve)).unwrap();
        let text = plan.config.to_string();
        assert!(text.contains(PEER_KEY_PLACEHOLDER));
        assert!(!text.contains("PRIVATE"));
    }
}
