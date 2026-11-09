//! Apply -> Verify -> Receipt (ADR-0022).
//!
//! Apply performs only the two local, file-level setup stages that the CLI already offers
//! (`setup init`, `setup join`); it never starts a service, changes a firewall or contacts the
//! peer. Every stage is gated, verified from the files it wrote, rolled back (only the files
//! this apply created) when verification fails, and ends in a durable receipt that contains no
//! secret. The same idempotency key returns the stored receipt instead of acting twice.

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::setup::{self, PlanRequest};

pub const PEER_BUNDLE_FILE: &str = "peer-bundle.json";
const OWN_BUNDLE_FILE: &str = "bundle.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Prepare this node: node key (Noise) or PKI check (TLS), pairing state, public bundle.
    Init,
    /// Verify the peer's bundle and write the final configuration.
    Join,
    /// Ask systemd (through the root control helper, ADR-0023) to enable `baft-tunnel@NAME` at boot
    /// and start it now, then verify; stop and disable it again if verification fails.
    Start,
    /// Stop `baft-tunnel@NAME` and disable it at boot, then verify that it is down.
    Stop,
    /// Stop, then start and enable again (same verification as `start`).
    Restart,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Join => "join",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
}

/// Result of the server-side preflight that gates `init`.
pub struct Gate {
    pub overall: String,
    pub checks: Vec<(String, String)>,
    pub blocked: Vec<String>,
}

#[derive(Clone)]
pub struct ApplyRequest {
    pub stage: Stage,
    pub plan: PlanRequest,
    pub key: String,
    pub bundle: Option<String>,
    pub checksum: Option<String>,
}

pub fn valid_key(key: &str) -> bool {
    (8..=64).contains(&key.len())
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn s(v: &impl serde::Serialize) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn request_summary(r: &ApplyRequest) -> Value {
    let p = &r.plan;
    json!({
        "stage": r.stage.as_str(),
        "engine": p.engine.as_str(),
        "role": s(&p.role),
        "direction": s(&p.direction),
        "node_id": p.node_id,
        "peer_id": p.peer_id,
        "tunnel_id": p.tunnel_id,
        "service_port": p.service_port,
        "tunnel_port": p.tunnel_port,
        "peer_host": p.peer_host,
        "server_name": p.server_name,
        "dir": p.dir.to_string_lossy(),
        "instance": p.instance,
    })
}

/// Digest of what the caller asked for; the bundle is hashed, never stored.
fn request_digest(r: &ApplyRequest) -> String {
    let mut v = request_summary(r);
    v["bundle_sha256"] = json!(r.bundle.as_deref().map(|b| sha(b.as_bytes())));
    v["checksum"] = json!(r.checksum);
    sha(v.to_string().as_bytes())
}

fn check(id: &str, ok: bool, evidence: impl Into<String>) -> Value {
    json!({"id": id, "result": if ok { "pass" } else { "fail" }, "evidence": evidence.into()})
}

fn private_mode(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o007 == 0)
}

fn write_new(path: &Path, mode: u32, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

struct Tracked {
    name: String,
    kind: &'static str,
    /// Public content may be hashed in the receipt; key material never is.
    hashable: bool,
}

fn tracked(r: &ApplyRequest) -> Vec<Tracked> {
    let config = setup::config_file_name(r.plan.instance.as_deref())
        .unwrap_or_else(|_| "config.json".into());
    let t = |name: &str, kind, hashable| Tracked {
        name: name.into(),
        kind,
        hashable,
    };
    vec![
        t(setup::STATE_FILE, "pairing-state", true),
        t(OWN_BUNDLE_FILE, "public-bundle", true),
        t(setup::KEY_FILE, "node-key", false),
        t(&config, "config", true),
        t(PEER_BUNDLE_FILE, "peer-bundle", true),
    ]
}

/// What systemd reports about one instance (read-only; no privilege needed).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnitState {
    pub active: String,
    pub sub: String,
    pub restarts: u32,
    pub result: String,
    pub main_pid: u32,
    /// `UnitFileState`: `enabled` means the instance starts at boot.
    pub enabled: String,
}

/// Control of `baft-tunnel@NAME`. The real implementation only drops a request file that the
/// root-owned path unit turns into one fixed `systemctl` command (ADR-0023); tests use a fake.
pub trait Units {
    fn request(&self, name: &str, verb: &str) -> Result<(), String>;
    fn state(&self, name: &str) -> UnitState;
    fn settle(&self, d: std::time::Duration) {
        std::thread::sleep(d);
    }
    /// The session indicator written by the running unit (see `session_status`): `Some` only when
    /// the file is valid, says "authorized" and belongs to the unit's current main process.
    fn session(&self, _name: &str, _main_pid: u32) -> Option<Value> {
        None
    }
}

pub const CONTROL_DIR: &str = "/run/baft-tunnel/control";

pub struct SystemUnits {
    pub control_dir: PathBuf,
}

impl Default for SystemUnits {
    fn default() -> Self {
        Self {
            control_dir: PathBuf::from(CONTROL_DIR),
        }
    }
}

pub fn valid_instance(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl Units for SystemUnits {
    fn request(&self, name: &str, verb: &str) -> Result<(), String> {
        if !valid_instance(name) || !matches!(verb, "start" | "stop" | "enable" | "disable") {
            return Err("invalid instance or verb".into());
        }
        if !self.control_dir.is_dir() {
            return Err(format!(
                "the agent control helper is not installed ({} is missing); run scripts/install-agent-control.sh --instance {name}",
                self.control_dir.display()
            ));
        }
        write_new(&self.control_dir.join(format!("{name}.{verb}")), 0o660, b"")
            .map_err(|e| format!("cannot write the {verb} request: {e}"))
    }

    fn state(&self, name: &str) -> UnitState {
        let out = std::process::Command::new("systemctl")
            .args([
                "show",
                &format!("baft-tunnel@{name}.service"),
                "-p",
                "ActiveState",
                "-p",
                "SubState",
                "-p",
                "NRestarts",
                "-p",
                "Result",
                "-p",
                "MainPID",
                "-p",
                "UnitFileState",
                "--no-pager",
            ])
            .output();
        let mut st = UnitState {
            active: "unknown".into(),
            ..UnitState::default()
        };
        if let Ok(o) = out {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                match line.split_once('=') {
                    Some(("ActiveState", v)) => st.active = v.into(),
                    Some(("SubState", v)) => st.sub = v.into(),
                    Some(("NRestarts", v)) => st.restarts = v.parse().unwrap_or(0),
                    Some(("Result", v)) => st.result = v.into(),
                    Some(("MainPID", v)) => st.main_pid = v.parse().unwrap_or(0),
                    Some(("UnitFileState", v)) => st.enabled = v.into(),
                    _ => {}
                }
            }
        }
        st
    }

    fn session(&self, name: &str, main_pid: u32) -> Option<Value> {
        if !valid_instance(name) || main_pid == 0 {
            return None;
        }
        let path = Path::new("/run/baft-tunnel")
            .join(name)
            .join("session.json");
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if !meta.is_file() || meta.len() > 4096 {
            return None;
        }
        let v: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
        (v["state"] == "authorized" && v["pid"].as_u64() == Some(u64::from(main_pid))).then_some(v)
    }
}

impl Stage {
    /// Stages that act on the systemd unit and write no files.
    pub fn is_lifecycle(self) -> bool {
        matches!(self, Self::Start | Self::Stop | Self::Restart)
    }
}

/// The stored receipt for this idempotency key: replayed when the request matches, else a conflict.
fn replay(stored: &Path, digest: &str) -> Option<(u16, Value)> {
    let text = std::fs::read_to_string(stored).ok()?;
    Some(match serde_json::from_str::<Value>(&text) {
        Ok(mut old) if old["request_sha256"] == json!(digest) => {
            old["replayed"] = json!(true);
            (200, old)
        }
        _ => (
            409,
            json!({"error": "idempotency key already used for a different request"}),
        ),
    })
}

fn base_receipt(key: &str, stage: Stage, request: Value, digest: &str) -> Value {
    json!({
        "receipt_version": 1,
        "id": key,
        "stage": stage.as_str(),
        "started_at": now(),
        "request": request,
        "request_sha256": digest,
        "replayed": false,
        "secrets": "none recorded: key material, the agent token and bundle contents are never written to a receipt",
    })
}

/// Start, stop or restart the instance the agent is bound to, without a setup plan (panel
/// lifecycle buttons). Same gates, verification, receipt and idempotency as Apply.
pub fn lifecycle(
    stage: Stage,
    instance: &str,
    dir: &Path,
    key: &str,
    receipts: &Path,
    units: &dyn Units,
) -> (u16, Value) {
    if !stage.is_lifecycle() {
        return (
            422,
            json!({"error": "only start, stop and restart act on a running instance"}),
        );
    }
    let request =
        json!({"stage": stage.as_str(), "instance": instance, "dir": dir.to_string_lossy()});
    let digest = sha(request.to_string().as_bytes());
    let stored = receipts.join(format!("{key}.json"));
    if let Some(old) = replay(&stored, &digest) {
        return old;
    }
    let receipt = base_receipt(key, stage, request, &digest);
    run_lifecycle(
        stage,
        Some(instance.to_string()),
        dir,
        receipt,
        &stored,
        receipts,
        units,
    )
}

/// Run one stage. Returns the HTTP status and the receipt.
pub fn run(r: &ApplyRequest, receipts: &Path, gate: Option<&Gate>) -> (u16, Value) {
    run_with(r, receipts, gate, &SystemUnits::default())
}

pub fn run_with(
    r: &ApplyRequest,
    receipts: &Path,
    gate: Option<&Gate>,
    units: &dyn Units,
) -> (u16, Value) {
    let digest = request_digest(r);
    let stored = receipts.join(format!("{}.json", r.key));
    if let Some(old) = replay(&stored, &digest) {
        return old;
    }
    let dir = r.plan.dir.clone();
    let mut receipt = base_receipt(&r.key, r.stage, request_summary(r), &digest);
    if r.stage.is_lifecycle() {
        return run_lifecycle(
            r.stage,
            r.plan.instance.clone(),
            &dir,
            receipt,
            &stored,
            receipts,
            units,
        );
    }
    if let Some(g) = gate {
        receipt["gate"] = json!({
            "source": "preflight re-run by the agent for this request",
            "overall": g.overall,
            "checks": g.checks.iter().map(|(i, r)| json!({"id": i, "result": r})).collect::<Vec<_>>(),
        });
        if !g.blocked.is_empty() {
            receipt["result"] = json!("refused");
            receipt["changes"] = json!([]);
            receipt["reason"] = json!(format!(
                "preflight blocked: {}; nothing was changed",
                g.blocked.join(", ")
            ));
            receipt["finished_at"] = json!(now());
            return (409, receipt);
        }
    }
    let files = tracked(r);
    let before: Vec<bool> = files
        .iter()
        .map(|f| dir.join(&f.name).symlink_metadata().is_ok())
        .collect();
    let mut pairing = Value::Null;
    let mut action_error: Option<String> = None;
    match r.stage {
        Stage::Init => match setup::init(&r.plan, None) {
            Ok(res) => {
                pairing = json!({"bundle_path": res.bundle_path.to_string_lossy(), "checksum": res.checksum});
            }
            Err(e) => action_error = Some(format!("{e:#}")),
        },
        Stage::Join => action_error = join(r, &dir).err(),
        Stage::Start | Stage::Stop | Stage::Restart => {
            unreachable!("lifecycle stages are handled by run_lifecycle")
        }
    }
    let created: Vec<(&Tracked, PathBuf)> = files
        .iter()
        .zip(&before)
        .filter(|(f, was)| !**was && dir.join(&f.name).symlink_metadata().is_ok())
        .map(|(f, _)| (f, dir.join(&f.name)))
        .collect();
    let mut verification: Vec<Value> = Vec::new();
    if let Some(error) = &action_error {
        verification.push(check("action", false, error.clone()));
    } else {
        verification = verify(r, &dir, &pairing, &created);
    }
    let failed = verification.iter().any(|v| v["result"] != "pass");
    let changes: Vec<Value> = created
        .iter()
        .map(|(f, path)| {
            let mode = std::fs::metadata(path)
                .map(|m| format!("{:04o}", m.permissions().mode() & 0o7777))
                .unwrap_or_default();
            let mut c = json!({"path": path.to_string_lossy(), "kind": f.kind, "action": "created", "mode": mode});
            if f.hashable {
                if let Ok(bytes) = std::fs::read(path) {
                    c["sha256"] = json!(sha(&bytes));
                }
            }
            c
        })
        .collect();
    receipt["changes"] = json!(changes);
    receipt["verification"] = json!(verification);
    if failed {
        // Roll back only what this apply created; pre-existing files are never touched.
        let mut removed = Vec::new();
        let mut left = Vec::new();
        for (_, path) in &created {
            match std::fs::remove_file(path) {
                Ok(()) => removed.push(path.to_string_lossy().into_owned()),
                Err(e) => {
                    left.push(json!({"path": path.to_string_lossy(), "error": e.to_string()}))
                }
            }
        }
        receipt["result"] = json!("rolled_back");
        receipt["rollback"] = json!({"performed": true, "removed": removed, "not_removed": left});
    } else {
        receipt["result"] = json!("applied");
        receipt["rollback"] = json!({
            "performed": false,
            "how_to_undo": "delete the files listed under changes; they were created by this apply and nothing else was modified",
        });
        if !pairing.is_null() {
            receipt["pairing"] = pairing;
        }
        receipt["not_done"] = json!(not_done(r));
        receipt["next_commands"] = json!(next_commands(r));
    }
    receipt["finished_at"] = json!(now());
    persist(receipts, &stored, &mut receipt);
    (if failed { 500 } else { 200 }, receipt)
}

fn join(r: &ApplyRequest, dir: &Path) -> Result<(), String> {
    let (Some(bundle), Some(checksum)) = (&r.bundle, &r.checksum) else {
        return Err("join needs the peer bundle and its checksum".into());
    };
    if bundle.len() as u64 > setup::BUNDLE_MAX_BYTES {
        return Err("bundle is too large".into());
    }
    let path = dir.join(PEER_BUNDLE_FILE);
    write_new(&path, 0o644, bundle.as_bytes())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    setup::join(dir, &path, checksum)
        .map(|_| ())
        .map_err(|e| format!("{e:#}"))
}

fn verify(
    r: &ApplyRequest,
    dir: &Path,
    pairing: &Value,
    created: &[(&Tracked, PathBuf)],
) -> Vec<Value> {
    let mut v = Vec::new();
    match r.stage {
        Stage::Start | Stage::Stop | Stage::Restart => {}
        Stage::Init => {
            let state = dir.join(setup::STATE_FILE);
            let state_json: Option<Value> = std::fs::read(&state)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
            v.push(check(
                "pairing_state",
                state_json.as_ref().is_some_and(|j| {
                    j["node_id"] == json!(r.plan.node_id)
                        && j["tunnel_id"] == json!(r.plan.tunnel_id)
                }) && private_mode(&state),
                "state file parses, names this node and tunnel, and is not world-accessible",
            ));
            let bundle_path = dir.join(OWN_BUNDLE_FILE);
            let bundle_bytes = std::fs::read(&bundle_path).unwrap_or_default();
            let parsed: Option<setup::Bundle> = serde_json::from_slice(&bundle_bytes).ok();
            v.push(check(
                "public_bundle",
                parsed.as_ref().is_some_and(|b| {
                    Some(b.checksum()) == pairing["checksum"].as_str().map(str::to_string)
                        && b.public.len() == 64
                        && hex::decode(&b.public).is_ok()
                }),
                "bundle parses, its checksum equals the one reported, and it holds a 64-hex public value",
            ));
            if r.plan.engine == crate::config::EngineKind::NoiseTcp {
                let key = dir.join(setup::KEY_FILE);
                let key_text = std::fs::read_to_string(&key).unwrap_or_default();
                v.push(check(
                    "node_key",
                    private_mode(&key) && !key_text.trim().is_empty(),
                    "node key exists as a regular file and is not group/world accessible",
                ));
                let leak = key_text.trim().len() >= 16
                    && String::from_utf8_lossy(&bundle_bytes).contains(key_text.trim());
                v.push(check(
                    "no_private_material_in_bundle",
                    !leak,
                    "the public bundle does not contain the node key",
                ));
            } else {
                v.push(check(
                    "pki_untouched",
                    created.iter().all(|(f, _)| f.kind != "node-key"),
                    "tcp-tls: this apply created no key material (PKI files are the operator's)",
                ));
            }
        }
        Stage::Join => {
            let name = setup::config_file_name(r.plan.instance.as_deref()).unwrap_or_default();
            let config = dir.join(&name);
            let loaded = crate::config::load(&config);
            v.push(check(
                "config_loads_and_validates",
                loaded.as_ref().is_ok_and(|c| c.validate().is_ok()),
                match &loaded {
                    Ok(c) => match c.validate() {
                        Ok(()) => {
                            "the written configuration loads and passes validation".to_string()
                        }
                        Err(e) => format!("validation failed: {e:#}"),
                    },
                    Err(e) => format!("{e:#}"),
                },
            ));
            v.push(check(
                "config_engine_matches",
                loaded
                    .as_ref()
                    .is_ok_and(|c| c.engine_name() == r.plan.engine.as_str()),
                "the configuration uses the engine that was requested",
            ));
            v.push(check(
                "config_mode",
                private_mode(&config),
                "configuration is not world-accessible",
            ));
            let kept = std::fs::read(dir.join(PEER_BUNDLE_FILE)).ok();
            v.push(check(
                "peer_bundle_recorded",
                kept.as_deref().map(sha) == r.bundle.as_deref().map(|b| sha(b.as_bytes())),
                "the stored peer bundle is byte-identical to the one supplied",
            ));
        }
    }
    v
}

fn not_done(r: &ApplyRequest) -> Vec<&'static str> {
    let mut list = vec![
        "no service was started or restarted",
        "no firewall or system setting was changed or inspected",
        "end-to-end data flow was not tested (needs both nodes running)",
    ];
    if r.stage == Stage::Init {
        list.insert(
            0,
            "pairing is not complete: exchange bundles and run the join stage on both nodes",
        );
    }
    list
}

fn next_commands(r: &ApplyRequest) -> Vec<String> {
    let d = r.plan.dir.to_string_lossy();
    match (r.stage, &r.plan.instance) {
        (Stage::Init, _) => vec![
            "Compare the bundle checksum with the peer's over a separate channel, then run the join stage here with the peer's bundle and checksum.".into(),
        ],
        (Stage::Join, Some(name)) => vec![format!(
            "Start baft-tunnel@{name} from the panel (Tunnels, Start) or with the Apply stage start; it is then enabled at boot"
        )],
        (Stage::Join, None) => vec![format!("baft-tunnel apply --config {d}/config.json --confirm")],
        (_, _) => vec![],
    }
}

/// The receipt is durable evidence; if it cannot be stored the caller must know.
fn persist(receipts: &Path, stored: &Path, receipt: &mut Value) {
    if let Err(e) = std::fs::create_dir_all(receipts)
        .and_then(|()| std::fs::set_permissions(receipts, std::fs::Permissions::from_mode(0o700)))
        .and_then(|()| write_new(stored, 0o640, receipt.to_string().as_bytes()))
    {
        receipt["receipt_stored"] = json!(false);
        receipt["receipt_error"] = json!(e.to_string());
    } else {
        receipt["receipt_stored"] = json!(true);
    }
}

fn tcp_open(addr: &str) -> bool {
    use std::net::{SocketAddr, TcpStream};
    let Ok(mut sa) = addr.parse::<SocketAddr>() else {
        return false;
    };
    if sa.ip().is_unspecified() {
        sa.set_ip(std::net::Ipv4Addr::LOCALHOST.into());
    }
    TcpStream::connect_timeout(&sa, std::time::Duration::from_secs(2)).is_ok()
}

fn is_up(st: &UnitState) -> bool {
    matches!(st.active.as_str(), "active" | "activating" | "reloading")
}

/// Poll until `done` holds (at most about 20 s; systemd acts asynchronously on a request).
fn wait_for(units: &dyn Units, name: &str, done: impl Fn(&UnitState) -> bool) -> UnitState {
    let mut st = units.state(name);
    for _ in 0..40 {
        if done(&st) {
            break;
        }
        units.settle(std::time::Duration::from_millis(500));
        st = units.state(name);
    }
    st
}

fn boot_view(st: &UnitState) -> Value {
    json!({
        "enabled_at_boot": st.enabled == "enabled",
        "unit_file_state": if st.enabled.is_empty() { "unknown" } else { st.enabled.as_str() },
    })
}

#[allow(clippy::too_many_arguments)]
fn run_lifecycle(
    stage: Stage,
    instance: Option<String>,
    dir: &Path,
    mut receipt: Value,
    stored: &Path,
    receipts: &Path,
    units: &dyn Units,
) -> (u16, Value) {
    let refuse = |mut rc: Value, why: String, code: u16| {
        rc["result"] = json!("refused");
        rc["changes"] = json!([]);
        rc["reason"] = json!(format!("{why}; nothing was changed"));
        rc["finished_at"] = json!(now());
        (code, rc)
    };
    let Some(name) = instance.filter(|n| valid_instance(n)) else {
        return refuse(
            receipt,
            format!(
                "{} needs an instance name (letters, digits, '_' or '-')",
                stage.as_str()
            ),
            409,
        );
    };
    if dir != Path::new("/etc/baft-tunnel") && !cfg!(test) {
        return refuse(
            receipt,
            format!(
                "{} only manages instances under /etc/baft-tunnel",
                stage.as_str()
            ),
            409,
        );
    }
    let unit = format!("baft-tunnel@{name}.service");
    let before = units.state(&name);
    receipt["before"] = json!({
        "active_state": before.active,
        "main_pid": before.main_pid,
        "boot": boot_view(&before),
    });
    if stage == Stage::Stop {
        return run_stop(
            &name, &unit, &before, receipt, stored, receipts, units, refuse,
        );
    }
    match stage {
        Stage::Start if is_up(&before) => {
            return refuse(
                receipt,
                format!("{unit} is already {}; use restart", before.active),
                409,
            );
        }
        Stage::Restart if !is_up(&before) => {
            return refuse(
                receipt,
                format!("{unit} is not running ({}); use start", before.active),
                409,
            );
        }
        _ => {}
    }
    let config_path = dir.join(format!("{name}.json"));
    // Parse only: validate() reads the private key, which the agent cannot (SA-09). The unit
    // validates itself and fails closed; the verify checks below catch a unit that does not come up.
    let config = match crate::config::load(&config_path) {
        Ok(c) => c,
        Err(e) => return refuse(receipt, format!("configuration is not ready: {e:#}"), 409),
    };
    if let Err(e) = config.validate_public() {
        return refuse(receipt, format!("configuration is not valid: {e:#}"), 409);
    }
    let direction = config.direction_name();
    if !crate::capabilities::supports_direction(config.engine_name(), direction) {
        return refuse(
            receipt,
            format!(
                "{direction} is not offered for {} in this build",
                config.engine_name()
            ),
            409,
        );
    }
    let mut v = Vec::new();
    if stage == Stage::Restart {
        if let Err(e) = units.request(&name, "stop") {
            return refuse(receipt, e, 409);
        }
        let down = wait_for(units, &name, |st| !is_up(st));
        let ok = !is_up(&down);
        v.push(check(
            "stopped_before_start",
            ok,
            format!("ActiveState={} after the stop request", down.active),
        ));
        if !ok {
            receipt["verification"] = json!(v);
            receipt["result"] = json!("failed");
            receipt["changes"] =
                json!([{"kind": "service", "action": "stop requested", "unit": unit}]);
            receipt["reason"] = json!("the unit did not stop; nothing was started");
            receipt["ready"] = json!("unknown");
            receipt["finished_at"] = json!(now());
            persist(receipts, stored, &mut receipt);
            return (500, receipt);
        }
    }
    // `enable --now`: the unit starts now and again after every reboot (ADR-0023 amendment).
    if let Err(e) = units.request(&name, "enable") {
        return refuse(receipt, e, 409);
    }
    let st = wait_for(units, &name, |st| {
        st.active == "active" && st.sub == "running"
    });
    v.push(check(
        "unit_active",
        st.active == "active" && st.sub == "running",
        format!("ActiveState={} SubState={}", st.active, st.sub),
    ));
    if st.active == "active" && st.sub == "running" {
        units.settle(std::time::Duration::from_secs(3));
        let again = units.state(&name);
        v.push(check(
            "stable_3s",
            again.active == "active" && again.restarts == 0 && again.result != "failed",
            format!(
                "after 3 s: ActiveState={} NRestarts={} Result={}",
                again.active, again.restarts, again.result
            ),
        ));
        v.push(check(
            "enabled_at_boot",
            again.enabled == "enabled",
            format!("UnitFileState={}", again.enabled),
        ));
        if stage == Stage::Restart {
            v.push(check(
                "new_process",
                again.main_pid != 0 && again.main_pid != before.main_pid,
                format!("MainPID {} -> {}", before.main_pid, again.main_pid),
            ));
        }
        // A Reverse service node dials out and has no listener (ADR-0021): there is nothing to connect to.
        if let Some(c) = listener_check(config.listen_addr()) {
            v.push(c);
        }
    }
    let failed = v.iter().any(|c| c["result"] != "pass");
    // Authorized peer session: observed only if the unit itself reports one. Waiting for a peer is
    // not a failure, so a missing session stays Unknown and never turns the stage into a rollback.
    let mut session = None;
    if !failed {
        let pid = units.state(&name).main_pid;
        for _ in 0..20 {
            session = units.session(&name, pid);
            if session.is_some() {
                break;
            }
            units.settle(std::time::Duration::from_millis(500));
        }
    }
    let session_seen = session.is_some();
    receipt["verification"] = json!(v);
    receipt["stages"] = json!({
        "files_applied": "unknown (this stage does not write files)",
        "service_state": if failed { "failed" } else { "observed" },
        "authenticated_peer": if session_seen { "observed" } else { "unknown" },
        "authorized_session": if session_seen { "observed" } else { "unknown" },
        "data_path_integrity": "unknown",
        "rollback": if failed { "performed" } else { "not needed" },
    });
    if let Some(sess) = &session {
        receipt["session"] = json!({
            "authorized_sessions": sess["authorized_sessions"],
            "since_unix": sess["since_unix"],
            "tunnel_id": sess["tunnel_id"],
            "direction": sess["direction"],
        });
    }
    // Never "ready" from this stage: no observation of bytes crossing the tunnel exists here.
    receipt["ready"] = json!("unknown");
    let mut changes = Vec::new();
    if stage == Stage::Restart {
        changes.push(json!({"kind": "service", "action": "stopped", "unit": unit}));
    }
    changes.push(json!({"kind": "service", "action": "started and enabled at boot", "unit": unit}));
    receipt["changes"] = json!(changes);
    if failed {
        // Stop it and remove the boot link, so a broken unit neither loops now nor after a reboot.
        let stop = units.request(&name, "disable");
        let after = wait_for(units, &name, |st| !is_up(st));
        receipt["result"] = json!("rolled_back");
        receipt["rollback"] = json!({
            "performed": stop.is_ok(),
            "action": "stop and disable at boot requested",
            "error": stop.err(),
            "state_after": {"active": after.active, "sub": after.sub, "boot": boot_view(&after)},
        });
        receipt["boot"] = boot_view(&after);
    } else {
        receipt["result"] = json!("applied");
        receipt["boot"] = boot_view(&units.state(&name));
        receipt["rollback"] = json!({"performed": false, "how_to_undo": "run the stop stage (panel: Stop); it stops the unit and disables it at boot"});
        receipt["not_done"] = json!([
            "no firewall rule was changed or inspected; open the tunnel port by hand where needed",
            if session_seen {
                "an authorized peer session was observed; the peer node itself was not checked from here"
            } else {
                "no authorized peer session was observed yet (the peer may not be started, or may be unreachable); an accepted local connection proves nothing about the peer"
            },
            "the peer node was not verified from here; start and check it too",
            "start at boot is configured, not proven: no reboot was performed",
            "data integrity over the tunnel is Unknown: Apply does not move test data through it (the P0-5 acceptance run does)",
        ]);
    }
    receipt["finished_at"] = json!(now());
    persist(receipts, stored, &mut receipt);
    (if failed { 500 } else { 200 }, receipt)
}

#[allow(clippy::too_many_arguments)]
fn run_stop(
    name: &str,
    unit: &str,
    before: &UnitState,
    mut receipt: Value,
    stored: &Path,
    receipts: &Path,
    units: &dyn Units,
    refuse: impl Fn(Value, String, u16) -> (u16, Value),
) -> (u16, Value) {
    if !is_up(before) && before.enabled != "enabled" {
        return refuse(
            receipt,
            format!(
                "{unit} is already stopped ({}) and not enabled at boot",
                before.active
            ),
            409,
        );
    }
    // `disable --now`: stopped now and not started again at boot.
    if let Err(e) = units.request(name, "disable") {
        return refuse(receipt, e, 409);
    }
    let after = wait_for(units, name, |st| !is_up(st) && st.enabled != "enabled");
    let v = vec![
        check(
            "unit_inactive",
            !is_up(&after),
            format!("ActiveState={} SubState={}", after.active, after.sub),
        ),
        check(
            "not_enabled_at_boot",
            after.enabled != "enabled",
            format!("UnitFileState={}", after.enabled),
        ),
    ];
    let failed = v.iter().any(|c| c["result"] != "pass");
    receipt["verification"] = json!(v);
    receipt["changes"] =
        json!([{"kind": "service", "action": "stopped and disabled at boot", "unit": unit}]);
    receipt["boot"] = boot_view(&after);
    receipt["stages"] = json!({
        "files_applied": "unknown (this stage does not write files)",
        "service_state": if failed { "failed" } else { "stopped" },
    });
    receipt["ready"] = json!("unknown");
    receipt["result"] = json!(if failed { "failed" } else { "applied" });
    receipt["rollback"] = json!({"performed": false, "how_to_undo": "run the start stage (panel: Start); it starts the unit and enables it at boot"});
    receipt["not_done"] = json!([
        "the peer node was not changed; it reports no session until this node starts again",
        "no firewall rule was changed or inspected",
        "configuration and keys were not touched",
    ]);
    receipt["finished_at"] = json!(now());
    persist(receipts, stored, &mut receipt);
    (if failed { 500 } else { 200 }, receipt)
}

/// Listener check for a started unit; `None` when the node has no listener (Reverse service node).
fn listener_check(listen: &str) -> Option<serde_json::Value> {
    if listen.is_empty() {
        return None;
    }
    Some(check(
        "listener_accepting",
        tcp_open(listen),
        format!("a TCP connection to {listen} was accepted"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authorization::ConnectionDirection;
    use crate::config::EngineKind;
    use crate::setup::Role;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("baft-apply-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o750)).unwrap();
        d
    }

    fn req(dir: &Path, role: Role, stage: Stage, key: &str) -> ApplyRequest {
        ApplyRequest {
            stage,
            plan: PlanRequest {
                engine: EngineKind::NoiseTcp,
                role,
                direction: ConnectionDirection::Direct,
                node_id: if role == Role::Serve {
                    "abroad"
                } else {
                    "iran"
                }
                .into(),
                peer_id: if role == Role::Serve {
                    "iran"
                } else {
                    "abroad"
                }
                .into(),
                tunnel_id: "demo".into(),
                service_port: 8080,
                tunnel_port: 8443,
                peer_host: (role == Role::Forward).then(|| "203.0.113.7".to_owned()),
                server_name: None,
                dir: dir.to_path_buf(),
                instance: None,
            },
            key: key.into(),
            bundle: None,
            checksum: None,
        }
    }

    fn open_gate() -> Gate {
        Gate {
            overall: "unknown".into(),
            checks: vec![("planner".into(), "pass".into())],
            blocked: vec![],
        }
    }

    #[test]
    fn init_applies_verifies_and_writes_a_receipt_without_secrets() {
        let dir = tmp("init");
        let receipts = dir.join("receipts");
        let r = req(&dir, Role::Serve, Stage::Init, "key-init-0001");
        let (code, out) = run(&r, &receipts, Some(&open_gate()));
        assert_eq!(code, 200, "{out}");
        assert_eq!(out["result"], "applied");
        assert!(
            out["verification"]
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v["result"] == "pass")
        );
        let kinds: Vec<_> = out["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["kind"].as_str().unwrap().to_string())
            .collect();
        for k in ["pairing-state", "public-bundle", "node-key"] {
            assert!(kinds.contains(&k.to_string()), "{kinds:?}");
        }
        let key_change = out["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["kind"] == "node-key")
            .unwrap();
        assert!(key_change.get("sha256").is_none());
        assert_eq!(key_change["mode"], "0600");
        let key_text = std::fs::read_to_string(dir.join("node.noise")).unwrap();
        let stored = std::fs::read_to_string(receipts.join("key-init-0001.json")).unwrap();
        assert!(!stored.contains(key_text.trim()));
        assert!(!out.to_string().contains(key_text.trim()));
        assert_eq!(out["rollback"]["performed"], false);
        assert!(out["not_done"].as_array().unwrap().len() >= 3);
    }

    #[test]
    fn same_key_replays_and_a_different_request_conflicts() {
        let dir = tmp("idem");
        let receipts = dir.join("receipts");
        let r = req(&dir, Role::Serve, Stage::Init, "key-idem-0001");
        assert_eq!(run(&r, &receipts, Some(&open_gate())).0, 200);
        let (code, again) = run(&r, &receipts, Some(&open_gate()));
        assert_eq!(code, 200);
        assert_eq!(again["replayed"], true);
        let mut other = req(&dir, Role::Serve, Stage::Init, "key-idem-0001");
        other.plan.service_port = 9090;
        assert_eq!(run(&other, &receipts, Some(&open_gate())).0, 409);
    }

    #[test]
    fn a_blocked_gate_refuses_and_changes_nothing() {
        let dir = tmp("gate");
        let r = req(&dir, Role::Serve, Stage::Init, "key-gate-0001");
        let gate = Gate {
            overall: "blocked".into(),
            checks: vec![("direction_support".into(), "blocked".into())],
            blocked: vec!["direction_support".into()],
        };
        let (code, out) = run(&r, &dir.join("receipts"), Some(&gate));
        assert_eq!(code, 409);
        assert_eq!(out["result"], "refused");
        assert!(!dir.join("node.noise").exists() && !dir.join("pairing-state.json").exists());
        assert!(!dir.join("receipts").exists());
    }

    #[test]
    fn a_failed_action_rolls_back_only_what_it_created() {
        let dir = tmp("rollback");
        // A pre-existing bundle makes init fail after nothing else should be left behind.
        std::fs::write(dir.join("bundle.json"), b"{}").unwrap();
        let r = req(&dir, Role::Serve, Stage::Init, "key-roll-0001");
        let (code, out) = run(&r, &dir.join("receipts"), Some(&open_gate()));
        assert_eq!(code, 500, "{out}");
        assert_eq!(out["result"], "rolled_back");
        assert!(
            dir.join("bundle.json").exists(),
            "pre-existing file must survive"
        );
        assert!(!dir.join("node.noise").exists());
        assert!(!dir.join("pairing-state.json").exists());
    }

    #[test]
    fn join_pairs_two_nodes_and_verifies_the_final_configuration() {
        let a = tmp("join-a");
        let b = tmp("join-b");
        let ra = req(&a, Role::Serve, Stage::Init, "key-ja-init-1");
        let rb = req(&b, Role::Forward, Stage::Init, "key-jb-init-1");
        let (_, oa) = run(&ra, &a.join("receipts"), Some(&open_gate()));
        let (_, ob) = run(&rb, &b.join("receipts"), Some(&open_gate()));
        let _ca = oa["pairing"]["checksum"].as_str().unwrap().to_string();
        let cb = ob["pairing"]["checksum"].as_str().unwrap().to_string();
        let bundle_a = std::fs::read_to_string(a.join("bundle.json")).unwrap();
        let bundle_b = std::fs::read_to_string(b.join("bundle.json")).unwrap();
        let mut ja = req(&a, Role::Serve, Stage::Join, "key-ja-join-1");
        ja.bundle = Some(bundle_b.clone());
        ja.checksum = Some(cb);
        let (code, out) = run(&ja, &a.join("receipts"), None);
        assert_eq!(code, 200, "{out}");
        assert_eq!(out["result"], "applied");
        assert!(a.join("config.json").exists());
        // wrong checksum: refused by setup, rolled back, previous config untouched
        let mut jb = req(&b, Role::Forward, Stage::Join, "key-jb-join-1");
        jb.bundle = Some(bundle_a);
        jb.checksum = Some("00000000-00000000-00000000-00000000".to_string());
        let (code, out) = run(&jb, &b.join("receipts"), None);
        assert_eq!(code, 500, "{out}");
        assert_eq!(out["result"], "rolled_back");
        assert!(!b.join("config.json").exists());
        assert!(!b.join(PEER_BUNDLE_FILE).exists());
    }

    use std::sync::Mutex;

    struct Fake {
        st: Mutex<UnitState>,
        comes_up: bool,
        session: bool,
        log: Mutex<Vec<String>>,
    }

    impl Fake {
        fn new(comes_up: bool, active: &str) -> Self {
            Self {
                st: Mutex::new(UnitState {
                    active: active.into(),
                    sub: "x".into(),
                    result: "success".into(),
                    restarts: 0,
                    main_pid: 4242,
                    enabled: if active == "active" {
                        "enabled"
                    } else {
                        "disabled"
                    }
                    .into(),
                }),
                comes_up,
                session: false,
                log: Mutex::new(vec![]),
            }
        }
    }

    impl Units for Fake {
        fn request(&self, name: &str, verb: &str) -> Result<(), String> {
            self.log.lock().unwrap().push(format!("{verb} {name}"));
            let mut st = self.st.lock().unwrap();
            if matches!(verb, "start" | "enable") && self.comes_up && st.active != "active" {
                st.active = "active".into();
                st.sub = "running".into();
                st.main_pid += 1;
            }
            if verb == "enable" {
                st.enabled = "enabled".into();
            }
            if matches!(verb, "stop" | "disable") {
                st.active = "inactive".into();
                st.sub = "dead".into();
            }
            if verb == "disable" {
                st.enabled = "disabled".into();
            }
            Ok(())
        }
        fn state(&self, _: &str) -> UnitState {
            self.st.lock().unwrap().clone()
        }
        fn settle(&self, _: std::time::Duration) {}
        fn session(&self, _: &str, pid: u32) -> Option<Value> {
            let cur = self.st.lock().unwrap().main_pid;
            (self.session && pid == cur).then(|| json!({"state":"authorized","pid":cur,"authorized_sessions":1,"since_unix":1,"tunnel_id":"t","direction":"direct"}))
        }
    }

    /// A paired, validated serve instance named `edge` whose tunnel port the test controls.
    fn paired_instance(name: &str) -> (PathBuf, std::net::TcpListener) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let (a, b) = (tmp(&format!("{name}-a")), tmp(&format!("{name}-b")));
        let mk = |dir: &Path, role| {
            let mut r = req(dir, role, Stage::Init, "key-start-init1");
            r.plan.tunnel_port = port;
            r.plan.instance = Some("edge".into());
            r
        };
        let (ra, rb) = (mk(&a, Role::Serve), mk(&b, Role::Forward));
        let (_, oa) = run(&ra, &a.join("r"), Some(&open_gate()));
        let (_, ob) = run(&rb, &b.join("r"), Some(&open_gate()));
        for (dir, other, ck) in [(&a, &b, &ob), (&b, &a, &oa)] {
            let mut j = req(
                dir,
                if dir == &a {
                    Role::Serve
                } else {
                    Role::Forward
                },
                Stage::Join,
                "key-start-join1",
            );
            j.plan.tunnel_port = port;
            j.plan.instance = Some("edge".into());
            j.bundle = Some(std::fs::read_to_string(other.join("bundle.json")).unwrap());
            j.checksum = Some(ck["pairing"]["checksum"].as_str().unwrap().to_string());
            let (code, out) = run(&j, &dir.join("r"), None);
            assert_eq!(code, 200, "{out}");
        }
        (a, l)
    }

    fn start_req(dir: &Path, key: &str) -> ApplyRequest {
        let mut r = req(dir, Role::Serve, Stage::Start, key);
        r.plan.instance = Some("edge".into());
        r
    }

    #[test]
    fn a_node_without_a_listener_has_no_listener_check() {
        // Reverse service node (ADR-0021): it dials out, so Apply start must not demand a listener.
        assert!(listener_check("").is_none());
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        assert_eq!(listener_check(&addr).unwrap()["result"], "pass");
    }

    #[test]
    fn start_verifies_a_running_unit_and_lists_what_it_did_not_do() {
        let (dir, _listener) = paired_instance("st-ok");
        let units = Fake::new(true, "inactive");
        let (code, out) = run_with(
            &start_req(&dir, "key-start-0001"),
            &dir.join("r"),
            None,
            &units,
        );
        assert_eq!(code, 200, "{out}");
        assert_eq!(out["result"], "applied");
        let ids: Vec<_> = out["verification"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap().to_string())
            .collect();
        for id in [
            "unit_active",
            "stable_3s",
            "enabled_at_boot",
            "listener_accepting",
        ] {
            assert!(ids.contains(&id.to_string()), "{ids:?}");
        }
        assert!(out["not_done"].to_string().contains("firewall"));
        assert_eq!(*units.log.lock().unwrap(), vec!["enable edge"]);
        assert_eq!(out["boot"]["enabled_at_boot"], true);
        // No session indicator: the peer stays Unknown, and nothing is ever "ready" from Apply.
        assert_eq!(out["stages"]["authorized_session"], "unknown");
        assert_eq!(out["stages"]["authenticated_peer"], "unknown");
        assert_eq!(out["stages"]["data_path_integrity"], "unknown");
        assert_eq!(out["ready"], "unknown");
        assert!(
            out["not_done"]
                .to_string()
                .contains("no authorized peer session")
        );
    }

    #[test]
    fn start_records_an_observed_session_but_still_not_ready() {
        let (dir, _listener) = paired_instance("st-sess");
        let mut units = Fake::new(true, "inactive");
        units.session = true;
        let (code, out) = run_with(
            &start_req(&dir, "key-start-0003"),
            &dir.join("r"),
            None,
            &units,
        );
        assert_eq!(code, 200, "{out}");
        assert_eq!(out["stages"]["authorized_session"], "observed");
        assert_eq!(out["session"]["authorized_sessions"], 1);
        assert_eq!(out["stages"]["data_path_integrity"], "unknown");
        assert_eq!(out["ready"], "unknown");
    }

    #[test]
    fn start_that_does_not_come_up_is_stopped_again() {
        let (dir, _listener) = paired_instance("st-bad");
        let units = Fake::new(false, "inactive");
        let (code, out) = run_with(
            &start_req(&dir, "key-start-0002"),
            &dir.join("r"),
            None,
            &units,
        );
        assert_eq!(code, 500, "{out}");
        assert_eq!(out["result"], "rolled_back");
        assert_eq!(
            *units.log.lock().unwrap(),
            vec!["enable edge", "disable edge"]
        );
        assert_eq!(out["rollback"]["state_after"]["active"], "inactive");
        // A unit that failed verification is not left enabled at boot.
        assert_eq!(out["boot"]["enabled_at_boot"], false);
    }

    #[test]
    fn start_refuses_a_running_unit_a_missing_config_and_a_missing_helper() {
        let (dir, _listener) = paired_instance("st-ref");
        let running = Fake::new(true, "active");
        let (code, out) = run_with(
            &start_req(&dir, "key-start-0003"),
            &dir.join("r"),
            None,
            &running,
        );
        assert_eq!(code, 409);
        assert!(
            running.log.lock().unwrap().is_empty(),
            "must not request anything"
        );
        assert!(out["reason"].as_str().unwrap().contains("already"));
        let mut other = start_req(&dir, "key-start-0004");
        other.plan.instance = Some("nope".into());
        assert_eq!(
            run_with(&other, &dir.join("r"), None, &Fake::new(true, "inactive")).0,
            409
        );
        let no_helper = SystemUnits {
            control_dir: dir.join("missing"),
        };
        let (code, out) = run_with(
            &start_req(&dir, "key-start-0005"),
            &dir.join("r"),
            None,
            &no_helper,
        );
        assert_eq!(code, 409);
        assert!(
            out["reason"]
                .as_str()
                .unwrap()
                .contains("install-agent-control")
        );
    }

    #[test]
    fn real_units_write_only_start_or_stop_requests_for_valid_instances() {
        let dir = tmp("ctl");
        let u = SystemUnits {
            control_dir: dir.clone(),
        };
        assert!(u.request("edge", "start").is_ok());
        assert!(dir.join("edge.start").exists());
        assert!(
            u.request("edge", "start").is_err(),
            "an unconsumed request is not overwritten"
        );
        for bad in ["", "../x", "a b", "a.b", "-x"] {
            assert!(u.request(bad, "start").is_err(), "{bad}");
        }
        assert!(u.request("edge", "restart").is_err());
        assert!(u.request("edge", "enable").is_ok());
        assert!(u.request("edge", "disable").is_ok());
        assert!(u.request("edge", "mask").is_err());
    }

    #[test]
    fn stop_disables_at_boot_and_refuses_when_already_down() {
        let (dir, _listener) = paired_instance("stop");
        let units = Fake::new(true, "active");
        let (code, out) = lifecycle(
            Stage::Stop,
            "edge",
            &dir,
            "key-stop-0001",
            &dir.join("r"),
            &units,
        );
        assert_eq!(code, 200, "{out}");
        assert_eq!(out["result"], "applied");
        assert_eq!(*units.log.lock().unwrap(), vec!["disable edge"]);
        assert_eq!(out["boot"]["enabled_at_boot"], false);
        assert_eq!(out["before"]["boot"]["enabled_at_boot"], true);
        // Same key: replayed, nothing requested again.
        let (code, again) = lifecycle(
            Stage::Stop,
            "edge",
            &dir,
            "key-stop-0001",
            &dir.join("r"),
            &units,
        );
        assert_eq!((code, again["replayed"].clone()), (200, json!(true)));
        assert_eq!(units.log.lock().unwrap().len(), 1);
        // Already stopped and disabled: refused, nothing requested.
        let (code, out) = lifecycle(
            Stage::Stop,
            "edge",
            &dir,
            "key-stop-0002",
            &dir.join("r"),
            &units,
        );
        assert_eq!(code, 409, "{out}");
        assert!(out["reason"].as_str().unwrap().contains("already stopped"));
        assert_eq!(units.log.lock().unwrap().len(), 1);
        // A different request under a used key is a conflict.
        let (code, _) = lifecycle(
            Stage::Start,
            "edge",
            &dir,
            "key-stop-0001",
            &dir.join("r"),
            &units,
        );
        assert_eq!(code, 409);
    }

    #[test]
    fn restart_stops_then_starts_a_new_process_and_refuses_a_stopped_unit() {
        let (dir, _listener) = paired_instance("restart");
        let stopped = Fake::new(true, "inactive");
        let (code, out) = lifecycle(
            Stage::Restart,
            "edge",
            &dir,
            "key-rs-00001",
            &dir.join("r"),
            &stopped,
        );
        assert_eq!(code, 409, "{out}");
        assert!(out["reason"].as_str().unwrap().contains("use start"));
        assert!(stopped.log.lock().unwrap().is_empty());

        let units = Fake::new(true, "active");
        let (code, out) = lifecycle(
            Stage::Restart,
            "edge",
            &dir,
            "key-rs-00002",
            &dir.join("r"),
            &units,
        );
        assert_eq!(code, 200, "{out}");
        assert_eq!(out["result"], "applied");
        assert_eq!(*units.log.lock().unwrap(), vec!["stop edge", "enable edge"]);
        let ids: Vec<_> = out["verification"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap().to_string())
            .collect();
        for id in [
            "stopped_before_start",
            "unit_active",
            "enabled_at_boot",
            "new_process",
        ] {
            assert!(ids.contains(&id.to_string()), "{ids:?}");
        }
        assert_eq!(out["ready"], "unknown");
        // Init and join are not lifecycle actions.
        assert_eq!(
            lifecycle(
                Stage::Init,
                "edge",
                &dir,
                "key-rs-00003",
                &dir.join("r"),
                &units
            )
            .0,
            422
        );
    }

    #[test]
    fn key_format_is_restricted() {
        assert!(valid_key("abcdefgh-1_2"));
        assert!(!valid_key("short"));
        assert!(!valid_key("has space 12345"));
        assert!(!valid_key("../../etc/passwd1"));
    }
}
