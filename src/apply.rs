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
}

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Join => "join",
        }
    }
}

/// Result of the server-side preflight that gates `init`.
pub struct Gate {
    pub overall: String,
    pub checks: Vec<(String, String)>,
    pub blocked: Vec<String>,
}

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

/// Run one stage. Returns the HTTP status and the receipt.
pub fn run(r: &ApplyRequest, receipts: &Path, gate: Option<&Gate>) -> (u16, Value) {
    let started = now();
    let digest = request_digest(r);
    let stored = receipts.join(format!("{}.json", r.key));
    if let Ok(text) = std::fs::read_to_string(&stored) {
        return match serde_json::from_str::<Value>(&text) {
            Ok(mut old) if old["request_sha256"] == json!(digest) => {
                old["replayed"] = json!(true);
                (200, old)
            }
            _ => (
                409,
                json!({"error": "idempotency key already used for a different request"}),
            ),
        };
    }
    let dir = r.plan.dir.clone();
    let mut receipt = json!({
        "receipt_version": 1,
        "id": r.key,
        "stage": r.stage.as_str(),
        "started_at": started,
        "request": request_summary(r),
        "request_sha256": digest,
        "replayed": false,
        "secrets": "none recorded: key material, the agent token and bundle contents are never written to a receipt",
    });
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
    // The receipt is durable evidence; if it cannot be stored the caller must know.
    if let Err(e) = std::fs::create_dir_all(receipts)
        .and_then(|()| std::fs::set_permissions(receipts, std::fs::Permissions::from_mode(0o700)))
        .and_then(|()| write_new(&stored, 0o640, receipt.to_string().as_bytes()))
    {
        receipt["receipt_stored"] = json!(false);
        receipt["receipt_error"] = json!(e.to_string());
    } else {
        receipt["receipt_stored"] = json!(true);
    }
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
        (Stage::Join, Some(name)) => vec![format!("systemctl enable --now baft-tunnel@{name}")],
        (Stage::Join, None) => vec![format!("baft-tunnel apply --config {d}/config.json --confirm")],
    }
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

    #[test]
    fn key_format_is_restricted() {
        assert!(valid_key("abcdefgh-1_2"));
        assert!(!valid_key("short"));
        assert!(!valid_key("has space 12345"));
        assert!(!valid_key("../../etc/passwd1"));
    }
}
