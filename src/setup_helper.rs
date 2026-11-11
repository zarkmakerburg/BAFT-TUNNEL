//! Root setup helper (ADR-0026): the only way the unprivileged agent gets `setup init` / `setup
//! join` run for its installed instance.
//!
//! The agent writes one request file, `CONTROL_DIR/NAME.setup` (stage, idempotency key, the plan
//! fields and, for join, the peer bundle and its checksum). The root path unit
//! `baft-tunnel-control-setup@NAME.path` runs exactly `baft-tunnel setup helper --instance NAME`,
//! which reads and removes the request, refuses anything that is not a regular file of bounded
//! size or that names another instance or directory, and then runs the same Apply code as the
//! agent and the CLI (`apply::run_with` -> `setup::init` / `setup::join`, verification, rollback
//! of only what it created). The agent never sends file contents: the helper derives every file
//! from validated parameters, so it never installs agent-authored configuration. On success the
//! files get the fixed service owners and modes (`setup::adopt_for_service`). The receipt is
//! written to `CONTROL_DIR/NAME.setup-result` (root:baft-tunnel 0640), where the agent reads it.

use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::apply::{self, ApplyRequest, Stage};

/// Request and result files are small: the plan, a bundle of at most 4 KiB, a receipt.
pub const MAX_FILE_BYTES: u64 = 64 * 1024;
pub const REQUEST_SUFFIX: &str = "setup";
pub const RESULT_SUFFIX: &str = "setup-result";
/// The root helper's own receipt store (replay of the same key, evidence on the host).
pub const HELPER_RECEIPTS: &str = "/var/lib/baft-tunnel-control/setup-receipts";
/// How long the agent waits for the helper; init on tcp-tls checks PKI, join validates config.
pub const WAIT: Duration = Duration::from_secs(60);

fn ensure_private_receipt_dir(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.is_dir() || meta.file_type().is_symlink() {
                bail!("receipt path {} is not a real directory", path.display());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path)
                .with_context(|| format!("create receipt directory {}", path.display()))?;
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect receipt directory {}", path.display()));
        }
    }
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() || meta.uid() != unsafe { libc::geteuid() } {
        bail!(
            "receipt directory {} has an unexpected type or owner",
            path.display()
        );
    }
    if meta.mode() & 0o022 != 0 {
        bail!(
            "receipt directory {} is writable by group or others",
            path.display()
        );
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("secure receipt directory {}", path.display()))?;
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HelperRequest {
    stage: Stage,
    key: String,
    plan: crate::agent::PlanBody,
    bundle: Option<String>,
    checksum: Option<String>,
}

/// The request document the agent writes (no secret: the bundle is public).
pub fn request_document(r: &ApplyRequest) -> Value {
    let p = &r.plan;
    json!({
        "stage": r.stage.as_str(),
        "key": r.key,
        "plan": {
            "engine": p.engine.as_str(),
            "role": p.role,
            "direction": p.direction,
            "node_id": p.node_id,
            "peer_id": p.peer_id,
            "tunnel_id": p.tunnel_id,
            "service_port": p.service_port,
            "tunnel_port": p.tunnel_port,
            "peer_host": p.peer_host,
            "server_name": p.server_name,
            "dir": p.dir,
            "instance": p.instance,
        },
        "bundle": r.bundle,
        "checksum": r.checksum,
    })
}

/// Read a small regular file without following a symlink at the last component.
fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("open {} (symlinks are refused)", path.display()))?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    if meta.len() > MAX_FILE_BYTES {
        bail!("{} is larger than {MAX_FILE_BYTES} bytes", path.display());
    }
    let mut buf = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_FILE_BYTES {
        bail!("{} grew beyond {MAX_FILE_BYTES} bytes", path.display());
    }
    Ok(buf)
}

fn refused(key: Option<&str>, reason: String) -> Value {
    json!({
        "receipt_version": 1,
        "id": key,
        "result": "refused",
        "reason": reason,
        "changes": [],
        "source": "root setup helper",
        "secrets": "none recorded: key material, the agent token and bundle contents are never written to a receipt",
    })
}

/// Parse and check one request against the instance and directory this helper serves.
fn parse(bytes: &[u8], name: &str, config_dir: &Path) -> Result<ApplyRequest> {
    let req: HelperRequest =
        serde_json::from_slice(bytes).context("the setup request is not valid")?;
    if !matches!(req.stage, Stage::Init | Stage::Join) {
        bail!("the setup helper only runs the init and join stages");
    }
    if !apply::valid_key(&req.key) {
        bail!("invalid idempotency key");
    }
    let plan = req.plan.to_request();
    if plan.instance.as_deref() != Some(name) {
        bail!("the request names another instance; this helper only serves {name}");
    }
    if plan.dir != config_dir {
        bail!(
            "the request names another directory; this helper only writes {}",
            config_dir.display()
        );
    }
    if req.stage == Stage::Join && (req.bundle.is_none() || req.checksum.is_none()) {
        bail!("join needs the peer bundle and its checksum");
    }
    if req
        .bundle
        .as_ref()
        .is_some_and(|b| b.len() as u64 > crate::setup::BUNDLE_MAX_BYTES)
    {
        bail!("bundle is too large");
    }
    Ok(ApplyRequest {
        stage: req.stage,
        plan,
        key: req.key,
        bundle: req.bundle,
        checksum: req.checksum,
    })
}

/// Write the result so that only root and group `baft-tunnel` can read it. Never follows links:
/// a stale result (or a planted link) is removed first and the new file is created exclusively.
fn write_result(control_dir: &Path, name: &str, receipt: &Value) -> Result<()> {
    let path = control_dir.join(format!("{name}.{RESULT_SUFFIX}"));
    let tmp = control_dir.join(format!(".{name}.{RESULT_SUFFIX}.tmp"));
    for p in [&path, &tmp] {
        match std::fs::remove_file(p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("remove {}", p.display())),
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("create {}", tmp.display()))?;
    file.write_all(receipt.to_string().as_bytes())?;
    file.sync_all()?;
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        if let Some((_, gid)) = crate::setup::service_ids() {
            std::os::unix::fs::fchown(&file, None, Some(gid))?;
        }
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o640))?;
    drop(file);
    std::fs::rename(&tmp, &path).with_context(|| format!("rename to {}", path.display()))?;
    Ok(())
}

/// One helper run for `name` (the fixed command of `baft-tunnel-control-setup@NAME.service`).
pub fn run(name: &str, control_dir: &Path, config_dir: &Path, receipts: &Path) -> Result<Value> {
    if !apply::valid_instance(name) {
        bail!("invalid instance name");
    }
    let request_path = control_dir.join(format!("{name}.{REQUEST_SUFFIX}"));
    let bytes = read_bounded(&request_path);
    // The request is consumed whatever happens, so a bad file cannot re-trigger the unit.
    let _ = std::fs::remove_file(&request_path);
    let receipt = match bytes.and_then(|b| parse(&b, name, config_dir)) {
        Err(e) => {
            // Echo the key only when the document parsed far enough to have one.
            refused(None, format!("{e:#}"))
        }
        Ok(req) => {
            let instance_receipts = receipts.join(name);
            match ensure_private_receipt_dir(receipts)
                .and_then(|()| ensure_private_receipt_dir(&instance_receipts))
            {
                Err(error) => refused(
                    Some(&req.key),
                    format!("cannot prepare the root-owned receipt directory: {error:#}"),
                ),
                Ok(()) => {
                    let (_, mut receipt) = apply::run(&req, &instance_receipts, None);
                    receipt["source"] = json!("root setup helper");
                    if receipt["result"] == "applied" && receipt["replayed"] != json!(true) {
                        let config_file = crate::setup::config_file_name(Some(name))?;
                        match crate::setup::adopt_for_service(config_dir, &config_file) {
                            Ok(lines) => receipt["ownership"] = json!(lines),
                            Err(e) => receipt["ownership_error"] = json!(format!("{e:#}")),
                        }
                    }
                    receipt
                }
            }
        }
    };
    write_result(control_dir, name, &receipt)?;
    Ok(receipt)
}

/// Agent side: how a setup stage reaches root. Tests use a fake that runs in-process.
pub trait SetupHelper {
    /// `Ok(())` when requests can be submitted for `name`, else why not (shown in preflight).
    fn available(&self, name: &str) -> Result<(), String>;
    /// Submit the request and wait for the helper's receipt.
    fn submit(&self, name: &str, request: &Value) -> Result<Value, String>;
}

pub struct SystemSetupHelper {
    pub control_dir: PathBuf,
}

impl Default for SystemSetupHelper {
    fn default() -> Self {
        Self {
            control_dir: PathBuf::from(apply::CONTROL_DIR),
        }
    }
}

impl SetupHelper for SystemSetupHelper {
    fn available(&self, name: &str) -> Result<(), String> {
        let unit = Path::new("/etc/systemd/system/baft-tunnel-control-setup@.service");
        if !self.control_dir.is_dir() || !unit.is_file() {
            return Err(format!(
                "the root setup helper is not installed; run sudo scripts/install-agent-control.sh --instance {name}"
            ));
        }
        let c = std::ffi::CString::new(self.control_dir.as_os_str().as_encoded_bytes())
            .map_err(|_| "control path contains a NUL byte".to_string())?;
        // SAFETY: c is a valid NUL-terminated string; access(2) only reads it.
        if unsafe { libc::access(c.as_ptr(), libc::W_OK | libc::X_OK) } != 0 {
            return Err(format!(
                "the agent cannot write requests into {}",
                self.control_dir.display()
            ));
        }
        Ok(())
    }

    fn submit(&self, name: &str, request: &Value) -> Result<Value, String> {
        if !apply::valid_instance(name) {
            return Err("invalid instance".into());
        }
        self.available(name)?;
        let req_path = self.control_dir.join(format!("{name}.{REQUEST_SUFFIX}"));
        let res_path = self.control_dir.join(format!("{name}.{RESULT_SUFFIX}"));
        let _ = std::fs::remove_file(&res_path);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o660)
            .open(&req_path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    "another setup request for this instance is still pending".to_string()
                } else {
                    format!("cannot write the setup request: {e}")
                }
            })?;
        file.write_all(request.to_string().as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("cannot write the setup request: {e}"))?;
        drop(file);
        let started = Instant::now();
        while started.elapsed() < WAIT {
            std::thread::sleep(Duration::from_millis(200));
            if std::fs::symlink_metadata(&res_path).is_err() {
                continue;
            }
            let bytes = read_bounded(&res_path).map_err(|e| format!("{e:#}"))?;
            let _ = std::fs::remove_file(&res_path);
            return serde_json::from_slice(&bytes)
                .map_err(|e| format!("the setup helper's result is not valid JSON: {e}"));
        }
        let _ = std::fs::remove_file(&req_path);
        Err(format!(
            "the root setup helper did not answer within {} s; check that baft-tunnel-control-setup@{name}.path is enabled (sudo scripts/install-agent-control.sh --instance {name})",
            WAIT.as_secs()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_json(dir: &Path, instance: &str) -> Value {
        json!({"engine": "noise-tcp", "role": "serve", "direction": "direct", "node_id": "abroad",
            "peer_id": "iran", "tunnel_id": "demo", "service_port": 18080, "tunnel_port": 18443,
            "dir": dir, "instance": instance})
    }

    struct Tmp(PathBuf);
    impl Tmp {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn setup() -> (Tmp, PathBuf, PathBuf, PathBuf) {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let tmp = Tmp(std::env::temp_dir().join(format!("baft-helper-{}-{n}", std::process::id())));
        let _ = std::fs::remove_dir_all(tmp.path());
        let control = tmp.path().join("control");
        let conf = tmp.path().join("etc");
        let receipts = tmp.path().join("receipts");
        std::fs::create_dir_all(&control).unwrap();
        std::fs::create_dir_all(&conf).unwrap();
        (tmp, control, conf, receipts)
    }

    fn put(control: &Path, body: &Value) {
        std::fs::write(control.join("inst1.setup"), body.to_string()).unwrap();
    }

    #[test]
    fn init_runs_the_shared_apply_code_and_writes_a_result() {
        let (_t, control, conf, receipts) = setup();
        put(
            &control,
            &json!({"stage": "init", "key": "helper-key-1", "plan": plan_json(&conf, "inst1")}),
        );
        let out = run("inst1", &control, &conf, &receipts).unwrap();
        assert_eq!(out["result"], "applied", "{out}");
        assert_eq!(out["source"], "root setup helper");
        assert!(conf.join("node.noise").is_file());
        assert!(conf.join("bundle.json").is_file());
        assert!(!control.join("inst1.setup").exists(), "request consumed");
        let res: Value =
            serde_json::from_slice(&std::fs::read(control.join("inst1.setup-result")).unwrap())
                .unwrap();
        assert_eq!(res["id"], "helper-key-1");
        let mode = std::fs::metadata(control.join("inst1.setup-result"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640);
    }

    #[test]
    fn receipt_parent_failure_returns_refused_result_without_waiting_for_timeout() {
        let (_t, control, conf, receipts) = setup();
        put(
            &control,
            &json!({"stage": "init", "key": "helper-key-parent", "plan": plan_json(&conf, "inst1")}),
        );
        // The parent of the configured receipt root does not exist. The helper must still
        // publish a bounded refusal through the separate control result channel.
        let missing_parent = receipts.join("missing").join("setup-receipts");
        let out = run("inst1", &control, &conf, &missing_parent).unwrap();
        assert_eq!(out["result"], "refused", "{out}");
        assert!(
            out["reason"]
                .as_str()
                .unwrap()
                .contains("cannot prepare the root-owned receipt directory"),
            "{out}"
        );
        let result: Value =
            serde_json::from_slice(&std::fs::read(control.join("inst1.setup-result")).unwrap())
                .unwrap();
        assert_eq!(result["result"], "refused");
        assert_eq!(result["id"], "helper-key-parent");
        assert_eq!(std::fs::read_dir(&conf).unwrap().count(), 0);
    }

    #[test]
    fn refuses_other_instance_other_directory_and_lifecycle_stages() {
        let (_t, control, conf, receipts) = setup();
        for (body, why) in [
            (
                json!({"stage": "init", "key": "helper-key-2", "plan": plan_json(&conf, "other")}),
                "another instance",
            ),
            (
                json!({"stage": "init", "key": "helper-key-3", "plan": plan_json(Path::new("/tmp"), "inst1")}),
                "another directory",
            ),
            (
                json!({"stage": "start", "key": "helper-key-4", "plan": plan_json(&conf, "inst1")}),
                "only runs the init and join",
            ),
            (
                json!({"stage": "join", "key": "helper-key-5", "plan": plan_json(&conf, "inst1")}),
                "bundle and its checksum",
            ),
            (
                json!({"stage": "init", "key": "helper-key-6", "plan": plan_json(&conf, "inst1"), "config": "{}"}),
                "not valid",
            ),
        ] {
            put(&control, &body);
            let out = run("inst1", &control, &conf, &receipts).unwrap();
            assert_eq!(out["result"], "refused", "{body}");
            assert!(out["reason"].as_str().unwrap().contains(why), "{out}");
        }
        assert_eq!(
            std::fs::read_dir(&conf).unwrap().count(),
            0,
            "nothing written"
        );
    }

    #[test]
    fn refuses_symlinked_and_oversized_requests() {
        let (t, control, conf, receipts) = setup();
        let target = t.path().join("elsewhere.json");
        std::fs::write(
            &target,
            json!({"stage": "init", "key": "helper-key-7", "plan": plan_json(&conf, "inst1")})
                .to_string(),
        )
        .unwrap();
        std::os::unix::fs::symlink(&target, control.join("inst1.setup")).unwrap();
        let out = run("inst1", &control, &conf, &receipts).unwrap();
        assert_eq!(out["result"], "refused");
        assert!(out["reason"].as_str().unwrap().contains("symlink"), "{out}");
        assert!(target.exists(), "the link target is not touched");
        std::fs::write(
            control.join("inst1.setup"),
            vec![b' '; MAX_FILE_BYTES as usize + 1],
        )
        .unwrap();
        let out = run("inst1", &control, &conf, &receipts).unwrap();
        assert_eq!(out["result"], "refused");
        assert_eq!(std::fs::read_dir(&conf).unwrap().count(), 0);
    }

    #[test]
    fn refuses_symlinked_receipt_root() {
        let (t, _control, _conf, receipts) = setup();
        let victim = t.path().join("victim");
        std::fs::create_dir(&victim).unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&victim, &receipts).unwrap();
        assert!(ensure_private_receipt_dir(&receipts).is_err());
        assert_eq!(
            std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }

    #[test]
    fn refuses_group_writable_receipt_root() {
        let (t, _control, _conf, receipts) = setup();
        std::fs::create_dir(&receipts).unwrap();
        std::fs::set_permissions(&receipts, std::fs::Permissions::from_mode(0o770)).unwrap();
        assert!(ensure_private_receipt_dir(&receipts).is_err());
        assert_eq!(
            std::fs::metadata(&receipts).unwrap().permissions().mode() & 0o777,
            0o770
        );
        drop(t);
    }

    #[test]
    fn a_planted_result_link_is_replaced_not_followed() {
        let (t, control, conf, receipts) = setup();
        let victim = t.path().join("victim");
        std::fs::write(&victim, b"keep").unwrap();
        std::os::unix::fs::symlink(&victim, control.join("inst1.setup-result")).unwrap();
        put(
            &control,
            &json!({"stage": "init", "key": "helper-key-8", "plan": plan_json(&conf, "other")}),
        );
        run("inst1", &control, &conf, &receipts).unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        assert!(
            !std::fs::symlink_metadata(control.join("inst1.setup-result"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
