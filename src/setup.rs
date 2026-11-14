//! Guided setup, slice S1: build and validate a planned configuration without touching the host.
//! Nothing here reads or writes files, opens sockets or generates keys; later slices add that.

use std::{
    io::Write,
    net::IpAddr,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    authorization::{ConnectionDirection, PROTOCOL_V1, PROTOCOL_V2},
    config::EngineKind,
    handshake::validate_identity,
};

/// Placeholder written where pairing has not provided the peer's trust material yet.
pub const PEER_KEY_PLACEHOLDER: &str = "SET_BY_PAIRING";

pub const DEFAULT_TUNNEL_PORT: u16 = 8443;

fn default_protocol_version() -> u16 {
    PROTOCOL_V1
}

fn is_default_protocol_version(version: &u16) -> bool {
    *version == PROTOCOL_V1
}

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
    /// Both peers must be initialized with the same protocol generation; mixed versions are rejected.
    pub protocol_version: u16,
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
    /// Name of a systemd instance (`baft-tunnel@NAME`): the config is then written to `NAME.json`,
    /// the file that unit reads, instead of `config.json`.
    pub instance: Option<String>,
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
    if ![PROTOCOL_V1, PROTOCOL_V2].contains(&req.protocol_version) {
        bail!("protocol version must be 1 or 2");
    }
    if req.engine == EngineKind::Baft && req.protocol_version == PROTOCOL_V2 {
        bail!("protocol v2 is not supported by the external BAFT engine");
    }
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
    let reverse = req.direction == ConnectionDirection::Reverse;
    if reverse && req.engine == EngineKind::Baft {
        bail!("the baft engine is Preview, Direct only; it does not support reverse");
    }
    let x509 = uses_certificates(req.engine);
    match (x509, req.role, req.server_name.as_deref()) {
        (true, Role::Forward, Some(name)) => {
            validate_identity(name)?;
            if req.engine == EngineKind::Baft && name.contains('_') {
                bail!("server name for the baft engine must be ASCII letters, digits, '-' or '.'");
            }
        }
        (true, Role::Forward, None) => {
            bail!(
                "{} with role forward requires a server name",
                engine_name(req.engine)
            )
        }
        (_, _, Some(_)) => {
            bail!("server name is only used by tcp-tls or baft with role forward")
        }
        _ => {}
    }

    let path = |name: &str| req.dir.join(name);
    let text = |name: &str| path(name).to_string_lossy().into_owned();
    // Existing flat credential sets remain supported. New D2 enrollment uses a single
    // versioned directory pointer so CA, CRL and leaf certificate switch as one atomic unit.
    let versioned_tls = if x509 {
        crate::pki_helper::active_set_dir(&req.dir)?.is_some()
            || !["node.crt", "ca.crt", "ca.crl"]
                .into_iter()
                .all(|name| req.dir.join(name).is_file())
    } else {
        false
    };
    let mut config = json!({
        "engine": engine_name(req.engine),
        "direction": req.direction.as_str(),
        "protocol_version": req.protocol_version,
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
            if reverse {
                // Public node: the service port is the user-facing ingress (ADR-0021).
                map.insert(
                    "ingress".into(),
                    json!(format!("127.0.0.1:{}", req.service_port)),
                );
            } else {
                map.insert(
                    "target".into(),
                    json!(format!("127.0.0.1:{}", req.service_port)),
                );
            }
        }
        Role::Forward => {
            map.insert("mode".into(), json!("forward"));
            if reverse {
                // Service node: dials out and relays streams to the local service (ADR-0021).
                map.insert(
                    "target".into(),
                    json!(format!("127.0.0.1:{}", req.service_port)),
                );
            } else {
                map.insert(
                    "listen".into(),
                    json!(format!("127.0.0.1:{}", req.service_port)),
                );
            }
            let ip = peer_ip.expect("checked above");
            let remote = std::net::SocketAddr::new(ip, req.tunnel_port).to_string();
            map.insert("remote".into(), json!(remote));
            if let Some(name) = &req.server_name {
                map.insert("server_name".into(), json!(name));
            }
        }
    }

    let mut files = vec![PlannedFile {
        path: path(&config_file_name(req.instance.as_deref())?),
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
        EngineKind::TcpTls | EngineKind::Baft => {
            let ca = if versioned_tls {
                "pki/current/ca.crt"
            } else {
                "ca.crt"
            };
            let crl = if versioned_tls {
                "pki/current/ca.crl"
            } else {
                "ca.crl"
            };
            let cert = if versioned_tls {
                "pki/current/node.crt"
            } else {
                "node.crt"
            };
            map.insert("ca".into(), json!(text(ca)));
            map.insert("crl".into(), json!(text(crl)));
            map.insert("cert".into(), json!(text(cert)));
            map.insert("key".into(), json!(text("node.key")));
            if req.engine == EngineKind::Baft {
                // The pinned engine from the binary's own signed manifest, launched from the
                // root-owned store (`install.sh --baft-engine`); a store holding another
                // release is refused at start, never run.
                map.insert(
                    "baft".into(),
                    json!({
                        "sha256": crate::baft_engine::pinned_sha256()?,
                        "store": crate::baft_engine::STORE_DIR,
                    }),
                );
            }
            // A baft listener admits peers by CA, identity allowlist and revocation and takes
            // no pin; its dialer pins the listener certificate (one pin, no rollover list).
            let pins = req.engine == EngineKind::TcpTls || req.role == Role::Forward;
            if pins {
                map.insert("peer_fingerprint".into(), json!(PEER_KEY_PLACEHOLDER));
            }
            for (name, mode, note) in [
                (
                    ca,
                    "0644",
                    "trusted CA certificate(s), through the selected PKI set",
                ),
                (crl, "0644", "current CRL(s), through the selected PKI set"),
                (
                    cert,
                    "0644",
                    "this node's certificate, through the selected PKI set",
                ),
                ("node.key", "0600", "this node's private key"),
            ] {
                files.push(PlannedFile {
                    path: path(name),
                    mode,
                    note,
                });
            }
            if pins {
                pending.push(if req.engine == EngineKind::Baft {
                    "serving node's certificate SHA-256 pin (from the pairing bundle)"
                } else {
                    "peer certificate SHA-256 pin (one, or two during rollover)"
                });
            }
        }
    }
    Ok(SetupPlan {
        config,
        files,
        pending,
    })
}

// ---- Slice S2: `setup init` and `setup join` for noise-tcp -------------------------------

pub(crate) const BUNDLE_MAX_BYTES: u64 = 4096;
pub(crate) const STATE_FILE: &str = "pairing-state.json";
pub(crate) const KEY_FILE: &str = "node.noise";
const CONFIG_FILE: &str = "config.json";

fn default_config_file() -> String {
    CONFIG_FILE.into()
}

/// `config.json`, or `NAME.json` for a systemd instance (same name rules as `apply::valid_instance`).
pub fn config_file_name(instance: Option<&str>) -> Result<String> {
    let Some(name) = instance else {
        return Ok(CONFIG_FILE.into());
    };
    if !crate::apply::valid_instance(name) {
        bail!(
            "instance must be 1-63 ASCII letters, digits, '_' or '-' and start with an alphanumeric"
        );
    }
    Ok(format!("{name}.json"))
}

/// Public pairing bundle. It never contains a private key, a credential or a host address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub version: u8,
    pub tunnel_id: String,
    pub engine: String,
    pub direction: String,
    /// Omitted for v1 so legacy pairing bundle checksums remain byte-for-byte compatible.
    #[serde(
        default = "default_protocol_version",
        skip_serializing_if = "is_default_protocol_version"
    )]
    pub protocol_version: u16,
    pub role: Role,
    pub node_id: String,
    pub peer_id: String,
    pub service_port: u16,
    pub tunnel_port: u16,
    /// Noise public key, or the SHA-256 pin of the node certificate for tcp-tls.
    pub public: String,
    /// tcp-tls serve nodes: the name the forwarding peer must verify.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
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

pub(crate) fn same_checksum(a: &str, b: &str) -> bool {
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
    #[serde(default = "default_protocol_version")]
    protocol_version: u16,
    role: Role,
    node_id: String,
    peer_id: String,
    tunnel_id: String,
    service_port: u16,
    tunnel_port: u16,
    peer_host: Option<String>,
    own_public: String,
    #[serde(default = "default_config_file")]
    config_file: String,
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

#[derive(Debug)]
pub struct InitResult {
    pub bundle_path: PathBuf,
    pub checksum: String,
}

/// Prepare this node and write the public pairing bundle. Refuses to overwrite anything.
/// noise-tcp: generates the node key. tcp-tls: validates PKI material that already sits in
/// the setup directory (`node.key`, `node.crt`, `ca.crt`, `ca.crl` from `baft-pki.sh`).
pub fn init(req: &PlanRequest, bundle_out: Option<&Path>) -> Result<InitResult> {
    let mut checked = req.clone();
    let tls = uses_certificates(req.engine);
    if tls && req.role == Role::Forward && checked.server_name.is_none() {
        checked.server_name = Some("pending".into()); // learned from the peer bundle at join
    }
    plan(&checked)?; // full request validation, no side effects
    let dir = &req.dir;
    let config_file = config_file_name(req.instance.as_deref())?;
    for name in [config_file.as_str(), STATE_FILE] {
        if dir.join(name).exists() {
            bail!(
                "{} already exists; setup never overwrites",
                dir.join(name).display()
            );
        }
    }
    let bundle_path = bundle_out.map_or_else(|| dir.join("bundle.json"), Path::to_path_buf);
    if bundle_path.exists() {
        bail!(
            "{} already exists; setup never overwrites",
            bundle_path.display()
        );
    }
    let (public, server_name) = if tls {
        tls_material(dir, &req.node_id, req.engine)?
    } else {
        if dir.join(KEY_FILE).exists() {
            bail!(
                "{} already exists; setup never overwrites",
                dir.join(KEY_FILE).display()
            );
        }
        if !dir.exists() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o750)
                .create(dir)
                .with_context(|| format!("create {}", dir.display()))?;
        }
        (
            crate::engine::write_noise_keypair(&dir.join(KEY_FILE))?,
            None,
        )
    };
    let engine = engine_name(req.engine);
    let bundle = Bundle {
        version: 1,
        tunnel_id: req.tunnel_id.clone(),
        engine: engine.into(),
        direction: req.direction.as_str().into(),
        protocol_version: req.protocol_version,
        role: req.role,
        node_id: req.node_id.clone(),
        peer_id: req.peer_id.clone(),
        service_port: req.service_port,
        tunnel_port: req.tunnel_port,
        public: public.clone(),
        server_name: if req.role == Role::Serve {
            server_name
        } else {
            None
        },
    };
    let state = State {
        engine: engine.into(),
        direction: req.direction.as_str().into(),
        protocol_version: req.protocol_version,
        role: req.role,
        node_id: req.node_id.clone(),
        peer_id: req.peer_id.clone(),
        tunnel_id: req.tunnel_id.clone(),
        service_port: req.service_port,
        tunnel_port: req.tunnel_port,
        peer_host: req.peer_host.clone(),
        own_public: public,
        config_file: config_file.clone(),
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

/// The user and group that run `baft-tunnel@NAME` (created by `scripts/install.sh`).
pub const SERVICE_USER: &str = "baft-tunnel";

pub(crate) fn service_ids() -> Option<(u32, u32)> {
    let name = std::ffi::CString::new(SERVICE_USER).ok()?;
    // SAFETY: getpwnam/getgrnam read static buffers; we copy the ids out immediately and the
    // CLI is single-threaded at this point.
    unsafe {
        let pw = libc::getpwnam(name.as_ptr());
        let gr = libc::getgrnam(name.as_ptr());
        (!pw.is_null() && !gr.is_null()).then(|| ((*pw).pw_uid, (*gr).gr_gid))
    }
}

/// Run as root for an installed instance (`--instance`): hand the files `baft-tunnel@NAME` reads
/// to the service user, so no manual `chown`/`chgrp` is needed. The private key becomes
/// `baft-tunnel:baft-tunnel` with its mode kept at 0600; the configuration, pairing state and
/// public PKI files get group `baft-tunnel` and group read. Symlinks are never followed. Does
/// nothing (and returns an empty list) when not root.
pub fn adopt_for_service(dir: &Path, config_file: &str) -> Result<Vec<String>> {
    // SAFETY: geteuid has no preconditions and cannot fail.
    if unsafe { libc::geteuid() } != 0 {
        return Ok(vec![]);
    }
    let Some((uid, gid)) = service_ids() else {
        bail!(
            "the service user {SERVICE_USER} does not exist; run scripts/install.sh --binary first, then repeat this step"
        );
    };
    let mut done = Vec::new();
    let mut adopt = |name: &str, owner: Option<u32>, group_read: bool| -> Result<()> {
        let path = dir.join(name);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return Ok(());
        };
        if !meta.is_file() && !meta.is_dir() {
            bail!(
                "{} is not a regular file; refusing to change its owner",
                path.display()
            );
        }
        std::os::unix::fs::lchown(&path, owner, Some(gid))
            .with_context(|| format!("chown {}", path.display()))?;
        let mode = meta.permissions().mode() & 0o7777;
        let want = if group_read {
            mode | 0o040 | if meta.is_dir() { 0o010 } else { 0 }
        } else {
            mode
        };
        if want != mode {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(want))?;
        }
        done.push(format!(
            "{} -> {}:{SERVICE_USER} {:04o}",
            path.display(),
            if owner.is_some() {
                SERVICE_USER
            } else {
                "(owner kept)"
            },
            want
        ));
        Ok(())
    };
    // The directory itself only gets the group when setup created it (root-owned, no group yet).
    if std::fs::metadata(dir).is_ok_and(|m| m.uid() == 0 && m.gid() == 0) {
        adopt(".", None, true)?;
    }
    for key in [KEY_FILE, "node.key"] {
        adopt(key, Some(uid), false)?;
    }
    for public in [config_file, STATE_FILE, "ca.crt", "ca.crl", "node.crt"] {
        adopt(public, None, true)?;
    }
    Ok(done)
}

/// Engines whose nodes use X.509 material from the offline PKI (tcp-tls and baft).
pub(crate) fn uses_certificates(engine: EngineKind) -> bool {
    matches!(engine, EngineKind::TcpTls | EngineKind::Baft)
}

fn engine_name(engine: EngineKind) -> &'static str {
    match engine {
        EngineKind::TcpTls => "tcp-tls",
        EngineKind::NoiseTcp => "noise-tcp",
        EngineKind::Baft => "baft",
    }
}

/// Seconds of CRL validity setup requires; shorter fails closed before the runtime would warn.
const MIN_CRL_SECONDS: i64 = 24 * 3600;

/// Check existing offline-PKI output and return this node's certificate pin and server name.
fn tls_material(dir: &Path, node_id: &str, engine: EngineKind) -> Result<(String, Option<String>)> {
    use std::os::unix::fs::PermissionsExt;
    if !dir.is_dir() {
        bail!(
            "{} must exist and contain the baft-pki.sh output",
            dir.display()
        );
    }
    let selected_set = crate::pki_helper::active_set_dir(dir)?;
    let material_dir = selected_set.as_deref().unwrap_or(dir);
    for (base, name) in [
        (dir, "node.key"),
        (material_dir, "node.crt"),
        (material_dir, "ca.crt"),
        (material_dir, "ca.crl"),
    ] {
        let path = base.join(name);
        let meta = std::fs::symlink_metadata(&path).with_context(|| {
            format!(
                "{} is missing (produce it with scripts/pki/baft-pki.sh or install a versioned certificate set)",
                path.display()
            )
        })?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            bail!("{} must be a regular non-symlink file", path.display());
        }
    }
    let mode = std::fs::metadata(dir.join("node.key"))?
        .permissions()
        .mode()
        & 0o777;
    if mode & 0o077 != 0 {
        bail!("node.key must not be readable by group or others (use mode 0600)");
    }
    let (ca, crl, cert, key) = (
        material_dir.join("ca.crt"),
        material_dir.join("ca.crl"),
        material_dir.join("node.crt"),
        dir.join("node.key"),
    );
    crate::tls::server_config(&ca, &crl, &cert, &key)
        .context("node certificate, key, CA and CRL do not form a valid set")?;
    let remaining = crate::tls::crl_seconds_remaining(&crl)?;
    if remaining < MIN_CRL_SECONDS {
        bail!("CRL has under 24 hours left (or is expired); issue a fresh CRL first");
    }
    if engine == EngineKind::Baft {
        // Revalidate the selected BAFT profile at Init, not only when the panel imports
        // the certificate. Operators may have copied or restored PKI files manually.
        let expected_san = format!("DNS:{node_id}.baft-tunnel.invalid");
        crate::tls::verify_local_certificate(&ca, &crl, &cert, &expected_san)
            .context("BAFT certificate chain, EKU, validity or CRL verification failed")?;
        let certs = crate::tls::certificates(&cert)?;
        let leaf = certs
            .first()
            .context("BAFT certificate file contains no leaf certificate")?;
        crate::pki_helper::validate_leaf_policy(
            leaf.as_ref(),
            node_id,
            &expected_san,
            crate::pki_helper::IdentityProfile::Baft,
        )
        .context("BAFT certificate does not satisfy the exact identity/SAN profile")?;
        crate::baft_engine::certificate_summary(&cert, node_id)
    } else {
        crate::tls::node_certificate_summary(&cert, node_id)
    }
}

/// Verify the peer's bundle against this node's saved state and write the final config.
pub fn join(dir: &Path, bundle_path: &Path, checksum: &str) -> Result<PathBuf> {
    let state: State = serde_json::from_slice(
        &std::fs::read(dir.join(STATE_FILE)).context("read pairing state; run setup init first")?,
    )
    .context("pairing state is invalid")?;
    let config_path = dir.join(&state.config_file);
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
    if bundle.protocol_version != state.protocol_version {
        return Err(mismatch(
            "protocol generation; both nodes must use the same version",
        ));
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
    let engine = match state.engine.as_str() {
        "tcp-tls" => EngineKind::TcpTls,
        "baft" => EngineKind::Baft,
        _ => EngineKind::NoiseTcp,
    };
    let server_name = if uses_certificates(engine) && state.role == Role::Forward {
        Some(bundle.server_name.clone().context(
            "the serving node's bundle has no server name (its certificate needs a DNS or IPv4 SAN)",
        )?)
    } else {
        None
    };
    let req = PlanRequest {
        engine,
        role: state.role,
        direction: if state.direction == "reverse" {
            ConnectionDirection::Reverse
        } else {
            ConnectionDirection::Direct
        },
        protocol_version: state.protocol_version,
        node_id: state.node_id,
        peer_id: state.peer_id,
        tunnel_id: state.tunnel_id,
        service_port: state.service_port,
        tunnel_port: state.tunnel_port,
        peer_host: state.peer_host,
        server_name,
        dir: dir.to_path_buf(),
        instance: None,
    };
    let mut config = plan(&req)?.config;
    match engine {
        EngineKind::TcpTls => config["peer_fingerprint"] = json!(bundle.public),
        // The baft dialer pins the listener certificate; the listener takes no pin.
        EngineKind::Baft if state.role == Role::Forward => {
            config["peer_fingerprint"] = json!(bundle.public);
        }
        EngineKind::Baft => {}
        EngineKind::NoiseTcp => config["noise_peer_public"] = json!(bundle.public),
    }
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
            protocol_version: PROTOCOL_V1,
            node_id: "iran".into(),
            peer_id: "abroad".into(),
            tunnel_id: "demo".into(),
            service_port: 8080,
            tunnel_port: DEFAULT_TUNNEL_PORT,
            peer_host: (role == Role::Forward).then(|| "203.0.113.7".to_owned()),
            server_name: (engine == EngineKind::TcpTls && role == Role::Forward)
                .then(|| "abroad.lab".to_owned()),
            dir: PathBuf::from("/etc/baft-tunnel"),
            instance: None,
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
    fn protocol_v2_is_written_to_the_plan_for_supported_engines_and_external_baft_rejects_it() {
        for engine in [EngineKind::NoiseTcp, EngineKind::TcpTls] {
            let mut req = request(engine, Role::Forward);
            req.protocol_version = PROTOCOL_V2;
            let planned = plan(&req).unwrap();
            assert_eq!(planned.config["protocol_version"], PROTOCOL_V2);
            parses_as_runtime_config(&planned);
        }

        let mut external_baft = request(EngineKind::Baft, Role::Forward);
        external_baft.protocol_version = PROTOCOL_V2;
        assert!(plan(&external_baft).is_err());
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
    fn reverse_moves_the_ingress_to_the_listening_node_and_the_target_to_the_dialing_node() {
        for engine in [EngineKind::NoiseTcp, EngineKind::TcpTls] {
            let mut serve = request(engine, Role::Serve);
            serve.direction = ConnectionDirection::Reverse;
            let serve = plan(&serve).unwrap();
            assert_eq!(serve.config["ingress"], "127.0.0.1:8080");
            assert!(serve.config.get("target").is_none());
            parses_as_runtime_config(&serve);
            let mut forward = request(engine, Role::Forward);
            forward.direction = ConnectionDirection::Reverse;
            let forward = plan(&forward).unwrap();
            assert_eq!(forward.config["target"], "127.0.0.1:8080");
            assert!(forward.config.get("listen").is_none());
            assert_eq!(forward.config["remote"], "203.0.113.7:8443");
            parses_as_runtime_config(&forward);
        }
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
    fn instance_names_the_config_file_the_systemd_unit_reads() {
        assert_eq!(config_file_name(None).unwrap(), "config.json");
        assert_eq!(config_file_name(Some("edge-1")).unwrap(), "edge-1.json");
        for bad in [
            "",
            "../x",
            "a/b",
            ".hidden",
            "a.b",
            "a..b",
            "x y",
            &"a".repeat(64),
        ] {
            assert!(config_file_name(Some(bad)).is_err(), "{bad}");
        }
        let mut req = request(EngineKind::NoiseTcp, Role::Serve);
        req.instance = Some("edge".into());
        let planned = plan(&req).unwrap();
        assert!(planned.files.iter().any(|f| f.path.ends_with("edge.json")));
        assert!(
            !planned
                .files
                .iter()
                .any(|f| f.path.ends_with("config.json"))
        );
    }

    #[test]
    fn instance_pairing_writes_and_never_overwrites_named_config() {
        let root = temp("instance");
        let mut serve = side(&root, Role::Serve, "abroad", "iran", "serve");
        let mut forward = side(&root, Role::Forward, "iran", "abroad", "forward");
        serve.instance = Some("edge".into());
        forward.instance = Some("edge".into());
        let a = init(&serve, None).unwrap();
        let b = init(&forward, None).unwrap();
        let path = join(&forward.dir, &a.bundle_path, &a.checksum).unwrap();
        assert!(path.ends_with("edge.json"));
        assert!(!forward.dir.join("config.json").exists());
        assert!(join(&forward.dir, &a.bundle_path, &a.checksum).is_err());
        join(&serve.dir, &b.bundle_path, &b.checksum).unwrap();
        assert!(serve.dir.join("edge.json").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn protocol_generation_is_bound_to_pairing_bundle_and_mixed_join_is_rejected() {
        let root = temp("protocol-generation");
        let mut serve_v2 = side(&root, Role::Serve, "abroad", "iran", "serve-v2");
        let mut forward_v1 = side(&root, Role::Forward, "iran", "abroad", "forward-v1");
        serve_v2.protocol_version = PROTOCOL_V2;
        forward_v1.protocol_version = PROTOCOL_V1;
        let _serve = init(&serve_v2, None).unwrap();
        let forward = init(&forward_v1, None).unwrap();

        let legacy_bundle = std::fs::read_to_string(&forward.bundle_path).unwrap();
        assert!(!legacy_bundle.contains("protocol_version"));
        let mixed = join(&serve_v2.dir, &forward.bundle_path, &forward.checksum).unwrap_err();
        assert!(mixed.to_string().contains("protocol generation"));
        assert!(!serve_v2.dir.join(CONFIG_FILE).exists());

        let mut forward_v2 = side(&root, Role::Forward, "iran", "abroad", "forward-v2");
        forward_v2.protocol_version = PROTOCOL_V2;
        let peer = init(&forward_v2, None).unwrap();
        join(&serve_v2.dir, &peer.bundle_path, &peer.checksum).unwrap();
        let config: Value =
            serde_json::from_slice(&std::fs::read(serve_v2.dir.join(CONFIG_FILE)).unwrap())
                .unwrap();
        assert_eq!(config["protocol_version"], PROTOCOL_V2);
        std::fs::remove_dir_all(&root).unwrap();
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
    fn setup_never_overwrites_and_tls_without_pki_material_is_refused() {
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

    // ---- S3: tcp-tls from baft-pki.sh output ------------------------------------------------

    fn install_pki(
        root: &Path,
        dir_name: &str,
        node: &str,
        ca: &crate::tls::tests::TestCa,
    ) -> PathBuf {
        install_pki_uri(
            root,
            dir_name,
            node,
            ca,
            &format!("urn:baft-tunnel:node:{node}"),
        )
    }

    fn install_pki_uri(
        root: &Path,
        dir_name: &str,
        node: &str,
        ca: &crate::tls::tests::TestCa,
        uri: &str,
    ) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = root.join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        let (cert, key) = crate::tls::tests::leaf(&dir, &format!("{node}-gen"), uri, &ca.issuer);
        std::fs::rename(cert, dir.join("node.crt")).unwrap();
        std::fs::rename(key, dir.join("node.key")).unwrap();
        std::fs::set_permissions(dir.join("node.key"), std::fs::Permissions::from_mode(0o600))
            .unwrap();
        std::fs::copy(&ca.ca_pem, dir.join("ca.crt")).unwrap();
        std::fs::copy(&ca.crl_pem, dir.join("ca.crl")).unwrap();
        dir
    }

    fn tls_side(root: &Path, role: Role, node: &str, peer: &str, dir: PathBuf) -> PlanRequest {
        let mut req = side(root, role, node, peer, "unused");
        req.engine = EngineKind::TcpTls;
        req.server_name = None;
        req.dir = dir;
        req
    }

    #[test]
    fn tls_nodes_pair_from_existing_pki_output() {
        let root = temp("tls-pair");
        let ca = crate::tls::tests::make_ca(&root, "ca");
        let sd = install_pki(&root, "serve", "abroad", &ca);
        let fd = install_pki(&root, "forward", "iran", &ca);
        let a = init(
            &tls_side(&root, Role::Serve, "abroad", "iran", sd.clone()),
            None,
        )
        .unwrap();
        let b = init(
            &tls_side(&root, Role::Forward, "iran", "abroad", fd.clone()),
            None,
        )
        .unwrap();
        let serve_bundle: Bundle =
            serde_json::from_slice(&std::fs::read(&a.bundle_path).unwrap()).unwrap();
        let forward_bundle: Bundle =
            serde_json::from_slice(&std::fs::read(&b.bundle_path).unwrap()).unwrap();
        assert_eq!(serve_bundle.server_name.as_deref(), Some("abroad.baft-tunnel.invalid"));
        assert_eq!(forward_bundle.server_name, None);
        join(&sd, &b.bundle_path, &b.checksum).unwrap();
        join(&fd, &a.bundle_path, &a.checksum).unwrap();
        let load = |d: &Path| -> Value {
            serde_json::from_slice(&std::fs::read(d.join(CONFIG_FILE)).unwrap()).unwrap()
        };
        assert_eq!(load(&sd)["peer_fingerprint"], forward_bundle.public);
        assert_eq!(load(&fd)["peer_fingerprint"], serve_bundle.public);
        assert_eq!(load(&fd)["server_name"], "localhost");
        assert!(load(&sd).get("server_name").is_none());
        // The private key is never copied into the bundle or state.
        let key = std::fs::read_to_string(sd.join("node.key")).unwrap();
        let body: String = key.lines().filter(|l| !l.starts_with("-----")).collect();
        assert!(
            !std::fs::read_to_string(&a.bundle_path)
                .unwrap()
                .contains(&body)
        );
        assert!(
            !std::fs::read_to_string(sd.join(STATE_FILE))
                .unwrap()
                .contains(&body)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn baft_nodes_pair_with_the_pinned_store_engine_and_a_listener_pin_only() {
        let root = temp("baft-pair");
        let ca = crate::tls::tests::make_ca(&root, "ca");
        let sd = install_pki_uri(&root, "serve", "abroad", &ca, "urn:baft:node:abroad");
        let fd = install_pki_uri(&root, "forward", "iran", &ca, "urn:baft:node:iran");
        let baft_side = |role, node, peer, dir: &PathBuf| {
            let mut req = tls_side(&root, role, node, peer, dir.clone());
            req.engine = EngineKind::Baft;
            req
        };
        // Reverse and protocol v2 stay refused for this Preview engine.
        let mut rev = baft_side(Role::Serve, "abroad", "iran", &sd);
        rev.direction = ConnectionDirection::Reverse;
        assert!(plan(&rev).is_err());
        let a = init(&baft_side(Role::Serve, "abroad", "iran", &sd), None).unwrap();
        let b = init(&baft_side(Role::Forward, "iran", "abroad", &fd), None).unwrap();
        let serve_bundle: Bundle =
            serde_json::from_slice(&std::fs::read(&a.bundle_path).unwrap()).unwrap();
        assert_eq!(serve_bundle.engine, "baft");
        assert_eq!(serve_bundle.server_name.as_deref(), Some("localhost"));
        join(&sd, &b.bundle_path, &b.checksum).unwrap();
        join(&fd, &a.bundle_path, &a.checksum).unwrap();
        let load = |d: &Path| -> Value {
            serde_json::from_slice(&std::fs::read(d.join(CONFIG_FILE)).unwrap()).unwrap()
        };
        let (serve, forward) = (load(&sd), load(&fd));
        let pinned = crate::baft_engine::pinned_sha256().unwrap();
        for c in [&serve, &forward] {
            assert_eq!(c["engine"], "baft");
            assert_eq!(c["direction"], "direct");
            assert_eq!(c["baft"]["sha256"], pinned.as_str());
            assert_eq!(c["baft"]["store"], crate::baft_engine::STORE_DIR);
        }
        assert!(serve.get("peer_fingerprint").is_none());
        assert_eq!(forward["peer_fingerprint"], serve_bundle.public);
        assert_eq!(forward["server_name"], "abroad.baft-tunnel.invalid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn baft_init_refuses_a_tcp_tls_certificate_and_writes_nothing() {
        let root = temp("baft-wrong-uri");
        let ca = crate::tls::tests::make_ca(&root, "ca");
        let dir = install_pki_uri(
            &root,
            "serve",
            "abroad",
            &ca,
            "urn:baft-tunnel:node:abroad",
        );
        let mut req = tls_side(&root, Role::Serve, "abroad", "iran", dir.clone());
        req.engine = EngineKind::Baft;
        let error = init(&req, None).unwrap_err();
        assert!(
            format!("{error:#}").contains("urn:baft:node:abroad"),
            "{error:#}"
        );
        assert!(!dir.join(STATE_FILE).exists());
        assert!(!dir.join("bundle.json").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn versioned_certificate_set_is_used_by_setup_without_flat_file_migration() {
        let root = temp("tls-versioned");
        let ca = crate::tls::tests::make_ca(&root, "ca");
        let dir = install_pki(&root, "versioned-node", "abroad", &ca);
        let cert = std::fs::read(dir.join("node.crt")).unwrap();
        let ca_pem = std::fs::read(dir.join("ca.crt")).unwrap();
        let crl = std::fs::read(dir.join("ca.crl")).unwrap();
        let id = crate::pki_helper::version_set_id(&cert, &ca_pem, &crl);
        let set_dir = dir.join("pki").join("sets").join(&id);
        std::fs::create_dir_all(&set_dir).unwrap();
        for (name, bytes) in [
            ("node.crt", cert.as_slice()),
            ("ca.crt", ca_pem.as_slice()),
            ("ca.crl", crl.as_slice()),
        ] {
            std::fs::write(set_dir.join(name), bytes).unwrap();
            std::fs::remove_file(dir.join(name)).unwrap();
        }
        std::os::unix::fs::symlink(format!("sets/{id}"), dir.join("pki/current")).unwrap();

        let req = tls_side(&root, Role::Serve, "abroad", "iran", dir.clone());
        let plan = plan(&req).unwrap();
        assert_eq!(
            plan.config["ca"],
            dir.join("pki/current/ca.crt").to_string_lossy().as_ref()
        );
        assert_eq!(
            plan.config["crl"],
            dir.join("pki/current/ca.crl").to_string_lossy().as_ref()
        );
        assert_eq!(
            plan.config["cert"],
            dir.join("pki/current/node.crt").to_string_lossy().as_ref()
        );
        assert_eq!(
            plan.config["key"],
            dir.join("node.key").to_string_lossy().as_ref()
        );
        let result = init(&req, None).unwrap();
        assert!(result.bundle_path.is_file());
        assert!(
            !dir.join(config_file_name(req.instance.as_deref()).unwrap())
                .exists()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tls_init_refuses_incomplete_or_unsafe_material() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp("tls-bad");
        let ca = crate::tls::tests::make_ca(&root, "ca");
        let try_init = |dir: &Path, node: &str| {
            init(
                &tls_side(&root, Role::Serve, node, "iran", dir.to_path_buf()),
                None,
            )
        };
        // Missing CRL.
        let d = install_pki(&root, "nocrl", "abroad", &ca);
        std::fs::remove_file(d.join("ca.crl")).unwrap();
        assert!(try_init(&d, "abroad").is_err());
        // Key readable by others.
        let d = install_pki(&root, "openkey", "abroad", &ca);
        std::fs::set_permissions(d.join("node.key"), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        let err = try_init(&d, "abroad").unwrap_err().to_string();
        assert!(err.contains("0600"), "{err}");
        // Certificate belongs to a different node identity.
        let d = install_pki(&root, "wrongid", "mallory", &ca);
        assert!(try_init(&d, "abroad").is_err());
        // Missing directory.
        assert!(try_init(&root.join("absent"), "abroad").is_err());
        // CRL with under 24 hours left (an expired CRL is further below the same threshold).
        let d = install_pki(&root, "expired", "abroad", &ca);
        let expired = d.join("expired.crl");
        let status = std::process::Command::new("openssl")
            .args(["ca", "-gencrl", "-crlhours", "1", "-batch", "-config"])
            .arg(root.join("ca").join("openssl.cnf"))
            .arg("-out")
            .arg(&expired)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::rename(&expired, d.join("ca.crl")).unwrap();
        assert!(try_init(&d, "abroad").is_err());
        // Nothing was written by any refused attempt.
        for name in ["nocrl", "openkey", "wrongid", "expired"] {
            assert!(!root.join(name).join(STATE_FILE).exists());
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tls_pairing_refuses_engine_mismatch_with_a_noise_peer() {
        let root = temp("tls-mix");
        let ca = crate::tls::tests::make_ca(&root, "ca");
        let sd = install_pki(&root, "serve", "abroad", &ca);
        init(
            &tls_side(&root, Role::Serve, "abroad", "iran", sd.clone()),
            None,
        )
        .unwrap();
        let noise = side(&root, Role::Forward, "iran", "abroad", "noise");
        let n = init(&noise, None).unwrap();
        let err = join(&sd, &n.bundle_path, &n.checksum)
            .unwrap_err()
            .to_string();
        assert!(err.contains("engine"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
