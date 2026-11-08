//! Guided setup, slice S1: build and validate a planned configuration without touching the host.
//! Nothing here reads or writes files, opens sockets or generates keys; later slices add that.

use std::{
    io::Write,
    net::IpAddr,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{authorization::ConnectionDirection, config::EngineKind, handshake::validate_identity};

/// Placeholder written where pairing has not provided the peer's trust material yet.
pub const PEER_KEY_PLACEHOLDER: &str = "SET_BY_PAIRING";

pub const DEFAULT_TUNNEL_PORT: u16 = 8443;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum, Serialize, Deserialize)]
#[value(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
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

// ---- Slice S2: `setup init` and `setup join` for noise-tcp -------------------------------

const BUNDLE_MAX_BYTES: u64 = 4096;
const STATE_FILE: &str = "pairing-state.json";
const KEY_FILE: &str = "node.noise";
const CONFIG_FILE: &str = "config.json";

/// Public pairing bundle. It never contains a private key, a credential or a host address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub version: u8,
    pub tunnel_id: String,
    pub engine: String,
    pub direction: String,
    pub role: Role,
    pub node_id: String,
    pub peer_id: String,
    pub service_port: u16,
    pub tunnel_port: u16,
    pub public: String,
}

impl Bundle {
    /// SHA-256 over the canonical (fixed field order) JSON, shown in groups to compare by eye.
    pub fn checksum(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("bundle serializes");
        let digest = hex::encode(Sha256::digest(bytes));
        digest.as_bytes()[..32]
            .chunks(8)
            .map(|c| std::str::from_utf8(c).expect("hex is ascii"))
            .collect::<Vec<_>>()
            .join("-")
    }
}

fn same_checksum(a: &str, b: &str) -> bool {
    let norm = |x: &str| {
        x.chars()
            .filter(char::is_ascii_hexdigit)
            .collect::<String>()
            .to_ascii_lowercase()
    };
    let (a, b) = (norm(a), norm(b));
    a.len() == 32 && a == b
}

/// Everything `join` needs from this node, persisted by `init` (no secrets).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    engine: String,
    direction: String,
    role: Role,
    node_id: String,
    peer_id: String,
    tunnel_id: String,
    service_port: u16,
    tunnel_port: u16,
    peer_host: Option<String>,
    own_public: String,
}

fn write_new(path: &Path, mode: u32, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .with_context(|| format!("create {} (it must not already exist)", path.display()))?;
    file.write_all(bytes)?;
    Ok(())
}

pub struct InitResult {
    pub bundle_path: PathBuf,
    pub checksum: String,
}

/// Generate this node's key and write the public pairing bundle. Refuses to overwrite anything.
pub fn init(req: &PlanRequest, bundle_out: Option<&Path>) -> Result<InitResult> {
    if req.engine != EngineKind::NoiseTcp {
        bail!("guided setup currently supports noise-tcp only (tcp-tls arrives in slice S3)");
    }
    plan(req)?; // full request validation, no side effects
    let dir = &req.dir;
    for name in [KEY_FILE, CONFIG_FILE, STATE_FILE] {
        if dir.join(name).exists() {
            bail!(
                "{} already exists; setup never overwrites",
                dir.join(name).display()
            );
        }
    }
    if !dir.exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o750)
            .create(dir)
            .with_context(|| format!("create {}", dir.display()))?;
    }
    let bundle_path = bundle_out.map_or_else(|| dir.join("bundle.json"), Path::to_path_buf);
    if bundle_path.exists() {
        bail!(
            "{} already exists; setup never overwrites",
            bundle_path.display()
        );
    }
    let public = crate::engine::write_noise_keypair(&dir.join(KEY_FILE))?;
    let bundle = Bundle {
        version: 1,
        tunnel_id: req.tunnel_id.clone(),
        engine: "noise-tcp".into(),
        direction: req.direction.as_str().into(),
        role: req.role,
        node_id: req.node_id.clone(),
        peer_id: req.peer_id.clone(),
        service_port: req.service_port,
        tunnel_port: req.tunnel_port,
        public: public.clone(),
    };
    let state = State {
        engine: "noise-tcp".into(),
        direction: req.direction.as_str().into(),
        role: req.role,
        node_id: req.node_id.clone(),
        peer_id: req.peer_id.clone(),
        tunnel_id: req.tunnel_id.clone(),
        service_port: req.service_port,
        tunnel_port: req.tunnel_port,
        peer_host: req.peer_host.clone(),
        own_public: public,
    };
    write_new(
        &dir.join(STATE_FILE),
        0o640,
        &serde_json::to_vec_pretty(&state)?,
    )?;
    write_new(&bundle_path, 0o644, &serde_json::to_vec_pretty(&bundle)?)?;
    Ok(InitResult {
        checksum: bundle.checksum(),
        bundle_path,
    })
}

/// Verify the peer's bundle against this node's saved state and write the final config.
pub fn join(dir: &Path, bundle_path: &Path, checksum: &str) -> Result<PathBuf> {
    let state: State = serde_json::from_slice(
        &std::fs::read(dir.join(STATE_FILE)).context("read pairing state; run setup init first")?,
    )
    .context("pairing state is invalid")?;
    let config_path = dir.join(CONFIG_FILE);
    if config_path.exists() {
        bail!(
            "{} already exists; setup never overwrites",
            config_path.display()
        );
    }
    let meta = std::fs::metadata(bundle_path).context("read bundle")?;
    if !meta.is_file() || meta.len() > BUNDLE_MAX_BYTES {
        bail!("bundle must be a regular file of at most {BUNDLE_MAX_BYTES} bytes");
    }
    let bundle: Bundle = serde_json::from_slice(&std::fs::read(bundle_path)?)
        .context("bundle is not a valid pairing bundle")?;
    if !same_checksum(checksum, &bundle.checksum()) {
        bail!("bundle checksum does not match the value you provided; do not continue");
    }
    if bundle.version != 1 {
        bail!("unsupported bundle version");
    }
    let mismatch = |what: &str| anyhow::anyhow!("bundle {what} does not match this node's setup");
    if bundle.engine != state.engine {
        return Err(mismatch("engine"));
    }
    if bundle.direction != state.direction {
        return Err(mismatch("direction"));
    }
    if bundle.tunnel_id != state.tunnel_id {
        return Err(mismatch("tunnel id"));
    }
    if bundle.service_port != state.service_port {
        return Err(mismatch("service port"));
    }
    if bundle.tunnel_port != state.tunnel_port {
        return Err(mismatch("tunnel port"));
    }
    if bundle.node_id != state.peer_id {
        return Err(mismatch("node id"));
    }
    if bundle.peer_id != state.node_id {
        return Err(mismatch("peer id"));
    }
    if bundle.role == state.role {
        bail!("both nodes chose the same role; one must be serve and the other forward");
    }
    if bundle.public.len() != 64 || hex::decode(&bundle.public).is_err() {
        bail!("bundle public key must be 64 hex characters");
    }
    if bundle.public == state.own_public {
        bail!("bundle carries this node's own public key");
    }
    let req = PlanRequest {
        engine: EngineKind::NoiseTcp,
        role: state.role,
        direction: if state.direction == "reverse" {
            ConnectionDirection::Reverse
        } else {
            ConnectionDirection::Direct
        },
        node_id: state.node_id,
        peer_id: state.peer_id,
        tunnel_id: state.tunnel_id,
        service_port: state.service_port,
        tunnel_port: state.tunnel_port,
        peer_host: state.peer_host,
        server_name: None,
        dir: dir.to_path_buf(),
    };
    let mut config = plan(&req)?.config;
    config["noise_peer_public"] = json!(bundle.public);
    let runtime: crate::config::RuntimeConfig =
        serde_json::from_value(config.clone()).context("final configuration is invalid")?;
    runtime
        .validate()
        .context("final configuration failed validation")?;
    write_new(&config_path, 0o640, &serde_json::to_vec_pretty(&config)?)?;
    Ok(config_path)
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

    // ---- S2 -----------------------------------------------------------------------------

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "baft-setup-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn side(root: &Path, role: Role, node: &str, peer: &str, dir_name: &str) -> PlanRequest {
        let mut req = request(EngineKind::NoiseTcp, role);
        req.node_id = node.into();
        req.peer_id = peer.into();
        req.dir = root.join(dir_name);
        req
    }

    /// Returns (serve dir, forward dir, serve bundle, forward bundle, serve checksum, forward checksum).
    fn paired(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf, String, String) {
        let serve = side(root, Role::Serve, "abroad", "iran", "serve");
        let forward = side(root, Role::Forward, "iran", "abroad", "forward");
        let a = init(&serve, None).unwrap();
        let b = init(&forward, None).unwrap();
        (
            serve.dir,
            forward.dir,
            a.bundle_path,
            b.bundle_path,
            a.checksum,
            b.checksum,
        )
    }

    #[test]
    fn two_nodes_pair_and_produce_valid_consistent_configs() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp("pair");
        let (sd, fd, sb, fb, sc, fc) = paired(&root);
        join(&sd, &fb, &fc).unwrap();
        join(&fd, &sb, &sc).unwrap();
        let load = |d: &Path| -> Value {
            serde_json::from_slice(&std::fs::read(d.join(CONFIG_FILE)).unwrap()).unwrap()
        };
        let (serve_cfg, forward_cfg) = (load(&sd), load(&fd));
        let public = |bundle: &Path| -> String {
            serde_json::from_slice::<Bundle>(&std::fs::read(bundle).unwrap())
                .unwrap()
                .public
        };
        assert_eq!(serve_cfg["noise_peer_public"], public(&fb));
        assert_eq!(forward_cfg["noise_peer_public"], public(&sb));
        for dir in [&sd, &fd] {
            let mode = std::fs::metadata(dir.join(KEY_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bundle_contains_no_private_key_and_unknown_fields_are_refused() {
        let root = temp("secret");
        let (sd, fd, sb, fb, _sc, fc) = paired(&root);
        let private = std::fs::read_to_string(sd.join(KEY_FILE)).unwrap();
        let bundle_text = std::fs::read_to_string(&sb).unwrap();
        assert!(!bundle_text.contains(private.trim()));
        assert!(
            !std::fs::read_to_string(sd.join(STATE_FILE))
                .unwrap()
                .contains(private.trim())
        );
        // An extra field changes nothing silently: it is rejected.
        let mut value: Value =
            serde_json::from_str(&std::fs::read_to_string(&fb).unwrap()).unwrap();
        value["extra"] = json!("x");
        std::fs::write(&fb, value.to_string()).unwrap();
        assert!(join(&sd, &fb, &fc).is_err());
        let _ = fd;
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tampered_bundle_or_wrong_checksum_is_refused() {
        let root = temp("tamper");
        let (sd, _fd, _sb, fb, _sc, fc) = paired(&root);
        assert!(join(&sd, &fb, "00000000-00000000-00000000-00000000").is_err());
        let mut bundle: Bundle = serde_json::from_slice(&std::fs::read(&fb).unwrap()).unwrap();
        bundle.tunnel_port += 1;
        std::fs::write(&fb, serde_json::to_vec(&bundle).unwrap()).unwrap();
        let err = join(&sd, &fb, &fc).unwrap_err().to_string();
        assert!(err.contains("checksum"), "{err}");
        assert!(!sd.join(CONFIG_FILE).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mismatched_setups_are_refused_and_write_nothing() {
        let root = temp("mismatch");
        let serve = side(&root, Role::Serve, "abroad", "iran", "serve");
        let mut other = side(&root, Role::Forward, "iran", "abroad", "forward");
        other.direction = ConnectionDirection::Reverse;
        init(&serve, None).unwrap();
        let b = init(&other, None).unwrap();
        let err = join(&serve.dir, &b.bundle_path, &b.checksum)
            .unwrap_err()
            .to_string();
        assert!(err.contains("direction"), "{err}");

        let same_role = side(&root, Role::Serve, "iran", "abroad", "same");
        let c = init(&same_role, None).unwrap();
        let err = join(&serve.dir, &c.bundle_path, &c.checksum)
            .unwrap_err()
            .to_string();
        assert!(err.contains("role"), "{err}");

        let mut wrong_peer = side(&root, Role::Forward, "mallory", "abroad", "wp");
        wrong_peer.peer_id = "abroad".into();
        let d = init(&wrong_peer, None).unwrap();
        assert!(join(&serve.dir, &d.bundle_path, &d.checksum).is_err());
        assert!(!serve.dir.join(CONFIG_FILE).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn setup_never_overwrites_and_tls_is_not_yet_supported() {
        let root = temp("overwrite");
        let (sd, _fd, _sb, fb, _sc, fc) = paired(&root);
        let serve = side(&root, Role::Serve, "abroad", "iran", "serve");
        assert!(init(&serve, None).is_err());
        join(&sd, &fb, &fc).unwrap();
        assert!(join(&sd, &fb, &fc).is_err());
        let mut tls = side(&root, Role::Serve, "abroad", "iran", "tls");
        tls.engine = EngineKind::TcpTls;
        assert!(init(&tls, None).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn oversized_bundle_is_refused() {
        let root = temp("big");
        let (sd, _fd, _sb, fb, _sc, fc) = paired(&root);
        std::fs::write(&fb, vec![b' '; (BUNDLE_MAX_BYTES + 1) as usize]).unwrap();
        assert!(join(&sd, &fb, &fc).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn checksum_comparison_ignores_case_and_separators_only() {
        let root = temp("sum");
        let (_sd, _fd, sb, _fb, sc, _fc) = paired(&root);
        let bundle: Bundle = serde_json::from_slice(&std::fs::read(&sb).unwrap()).unwrap();
        assert!(same_checksum(
            &sc.to_uppercase().replace('-', " "),
            &bundle.checksum()
        ));
        assert!(!same_checksum("", &bundle.checksum()));
        assert!(!same_checksum(&sc[..sc.len() - 1], &bundle.checksum()));
        let _ = std::fs::remove_dir_all(&root);
    }
}
