//! Root tunnel management helper (ADR-0055): remove or edit the one instance an agent is bound to.
//!
//! The agent writes `CONTROL_DIR/NAME.manage` (`op`, idempotency `key`, and for `edit` only the
//! new peer addresses). The root path unit `baft-tunnel-control-manage@NAME.path` runs exactly
//! `baft-tunnel setup manage-helper --instance NAME`, which reads and removes the request and:
//!
//! - `remove`: refuses while `/run/baft-tunnel/NAME` exists (the unit is running), then moves
//!   `NAME.json` and, only when `pairing-state.json` names `NAME.json`, the pairing files and the
//!   generated Noise key into a root-only backup (`BACKUP_ROOT/NAME/KEY`, 0700). Operator PKI files
//!   (`node.key`, `node.crt`, `ca.crt`, `ca.crl`) are never touched.
//! - `edit`: changes only `remote` / `backup_remotes` of a forward (dialing) configuration, keeping
//!   the port, validates the result with the runtime schema, backs up the old files and replaces
//!   them atomically with the same owner and mode.
//! - `revert`: puts back exactly the files of one backup (rollback of a failed edit or remove);
//!   never overwrites a file that a remove backup would restore.
//!
//! The agent never sends file contents or paths. A failure part-way restores what was moved. The
//! receipt (`NAME.manage-result`, root:baft-tunnel 0640) lists file names only, never contents.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

pub const REQUEST_SUFFIX: &str = "manage";
pub const RESULT_SUFFIX: &str = "manage-result";
pub const BACKUP_ROOT: &str = "/var/lib/baft-tunnel-control/tunnel-backups";
pub const RUN_ROOT: &str = "/run/baft-tunnel";
const MAX_FILE_BYTES: u64 = 64 * 1024;
const MANIFEST: &str = "manifest.json";
/// Backups kept per instance; older ones are deleted after a successful operation.
const KEEP_BACKUPS: usize = 3;
pub const WAIT: Duration = Duration::from_secs(30);
/// Files that belong to an instance only when `pairing-state.json` names its config file.
const PAIRING_FILES: [&str; 4] = [
    crate::setup::STATE_FILE,
    "bundle.json",
    crate::apply::PEER_BUNDLE_FILE,
    crate::setup::KEY_FILE,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Remove,
    Edit,
    Revert,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Remove => "remove",
            Self::Edit => "edit",
            Self::Revert => "revert",
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub op: Op,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_peer_hosts: Option<Vec<String>>,
}

pub struct Dirs<'a> {
    pub control: &'a Path,
    pub config: &'a Path,
    pub backups: &'a Path,
    pub run: &'a Path,
}

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
    let mut buf = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_FILE_BYTES {
        bail!("{} is larger than {MAX_FILE_BYTES} bytes", path.display());
    }
    Ok(buf)
}

/// A regular file (not a link) at `path`: `Ok(Some(meta))`, absent: `Ok(None)`.
fn regular(path: &Path) -> Result<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => Ok(Some(m)),
        Ok(_) => bail!("{} is not a regular file; refusing", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("inspect {}", path.display())),
    }
}

fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

/// Create `path` exclusively with `bytes`, the mode, owner and group of `like`, fsynced.
fn write_like(path: &Path, bytes: &[u8], like: &std::fs::Metadata) -> Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    f.write_all(bytes)?;
    if is_root() {
        std::os::unix::fs::fchown(&f, Some(like.uid()), Some(like.gid()))?;
    }
    f.set_permissions(std::fs::Permissions::from_mode(like.mode() & 0o7777))?;
    f.sync_all()?;
    Ok(())
}

/// Copy `from` to the new file `to` (same mode and owner), then remove `from`. Works across
/// filesystems (`/etc` and `/var/lib` may differ), never follows links.
fn move_file(from: &Path, to: &Path) -> Result<()> {
    let meta = regular(from)?.with_context(|| format!("{} disappeared", from.display()))?;
    let bytes = read_bounded(from)?;
    write_like(to, &bytes, &meta)?;
    std::fs::remove_file(from).with_context(|| format!("remove {}", from.display()))?;
    Ok(())
}

/// Atomically replace `target` with `bytes`, keeping its owner and mode.
fn replace(target: &Path, bytes: &[u8], like: &std::fs::Metadata) -> Result<()> {
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .context("bad file name")?;
    let tmp = target.with_file_name(format!(".{name}.manage.tmp"));
    let _ = std::fs::remove_file(&tmp);
    write_like(&tmp, bytes, like)?;
    std::fs::rename(&tmp, target).with_context(|| format!("replace {}", target.display()))?;
    Ok(())
}

fn private_dir(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
        Ok(_) => bail!("{} is not a real directory", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(path)
                .with_context(|| format!("create {}", path.display()))?;
        }
        Err(e) => return Err(e).with_context(|| format!("inspect {}", path.display())),
    }
    let m = std::fs::symlink_metadata(path)?;
    // SAFETY: geteuid has no preconditions.
    if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0 {
        bail!("{} has an unexpected owner or mode", path.display());
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

/// Instance files `remove` would move, in order: the config first.
fn owned_files(config_dir: &Path, name: &str) -> Result<Vec<String>> {
    let config = format!("{name}.json");
    if regular(&config_dir.join(&config))?.is_none() {
        bail!("{config} does not exist; there is no tunnel to remove");
    }
    let mut files = vec![config.clone()];
    let state = config_dir.join(crate::setup::STATE_FILE);
    if regular(&state)?.is_some() {
        let v: Value = serde_json::from_slice(&read_bounded(&state)?).unwrap_or(Value::Null);
        if v["config_file"].as_str() == Some(config.as_str()) {
            for f in PAIRING_FILES {
                if regular(&config_dir.join(f))?.is_some() {
                    files.push(f.to_string());
                }
            }
        }
    }
    Ok(files)
}

fn valid_peer_ip(host: &str, what: &str) -> Result<IpAddr> {
    let ip: IpAddr = host
        .parse()
        .map_err(|_| anyhow::anyhow!("{what} must be an IP address"))?;
    let bad = ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_loopback()
        || matches!(ip, IpAddr::V4(v4) if v4.is_broadcast());
    if bad {
        bail!("{what} must be a unicast, non-loopback address");
    }
    Ok(ip)
}

/// The edited configuration (and pairing state when this instance owns it); validated, unwritten.
fn edited(
    config_dir: &Path,
    name: &str,
    peer_host: &str,
    backups: &[String],
) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
    let config_path = config_dir.join(format!("{name}.json"));
    let mut cfg: Value = serde_json::from_slice(&read_bounded(&config_path)?)
        .context("the current configuration is not valid JSON")?;
    let old_remote = cfg["remote"]
        .as_str()
        .context("only the dialing (forward) side has a peer address to edit")?;
    let port = old_remote
        .parse::<SocketAddr>()
        .context("the current remote is not an IP:port address")?
        .port();
    if backups.len() > crate::setup::MAX_BACKUP_HOSTS {
        bail!(
            "at most {} backup peer hosts",
            crate::setup::MAX_BACKUP_HOSTS
        );
    }
    let primary = valid_peer_ip(peer_host, "peer_host")?;
    let mut seen = vec![primary];
    let mut backup_addrs = Vec::new();
    for b in backups {
        let ip = valid_peer_ip(b, "backup peer host")?;
        if seen.contains(&ip) {
            bail!("backup peer hosts must be distinct and differ from the peer host");
        }
        seen.push(ip);
        backup_addrs.push(SocketAddr::new(ip, port).to_string());
    }
    let old = cfg.clone();
    cfg["remote"] = json!(SocketAddr::new(primary, port).to_string());
    let map = cfg
        .as_object_mut()
        .context("configuration is not an object")?;
    if backup_addrs.is_empty() {
        map.remove("backup_remotes");
    } else {
        map.insert("backup_remotes".into(), json!(backup_addrs));
    }
    if cfg == old {
        bail!("the requested peer addresses are already configured; nothing to change");
    }
    let runtime: crate::config::RuntimeConfig =
        serde_json::from_value(cfg.clone()).context("the edited configuration is not valid")?;
    runtime
        .validate_public()
        .context("the edited configuration is not valid")?;
    let state_path = config_dir.join(crate::setup::STATE_FILE);
    let state = if regular(&state_path)?.is_some() {
        let mut st: Value =
            serde_json::from_slice(&read_bounded(&state_path)?).unwrap_or(Value::Null);
        if st["config_file"].as_str() == Some(format!("{name}.json").as_str()) {
            st["peer_host"] = json!(primary.to_string());
            st["backup_peer_hosts"] = json!(
                seen[1..]
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            );
            Some(serde_json::to_vec_pretty(&st)?)
        } else {
            None
        }
    } else {
        None
    };
    Ok((serde_json::to_vec_pretty(&cfg)?, state))
}

fn write_manifest(dir: &Path, op: Op, files: &[String]) -> Result<()> {
    let body = serde_json::to_vec(&json!({"op": op.as_str(), "files": files}))?;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join(MANIFEST))?;
    f.write_all(&body)?;
    f.sync_all()?;
    Ok(())
}

/// Delete all but the newest `KEEP_BACKUPS` backups of this instance (never `keep`).
fn prune(instance_dir: &Path, keep: &Path) {
    let Ok(rd) = std::fs::read_dir(instance_dir) else {
        return;
    };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let m = std::fs::symlink_metadata(e.path()).ok()?;
            let t = m.modified().ok()?;
            (m.is_dir() && e.path() != keep).then(|| (t, e.path()))
        })
        .collect();
    dirs.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, d) in dirs.into_iter().skip(KEEP_BACKUPS.saturating_sub(1)) {
        let _ = std::fs::remove_dir_all(d);
    }
}

fn op_remove(name: &str, key: &str, d: &Dirs<'_>, backup: &Path) -> Result<Vec<String>> {
    if std::fs::symlink_metadata(d.run.join(name)).is_ok() {
        bail!(
            "baft-tunnel@{name} is still running ({}/{name} exists); stop it first",
            d.run.display()
        );
    }
    let files = owned_files(d.config, name)?;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(backup)
        .with_context(|| format!("backup for key {key} already exists or cannot be created"))?;
    let mut moved: Vec<&String> = Vec::new();
    for f in &files {
        if let Err(e) = move_file(&d.config.join(f), &backup.join(f)) {
            for m in moved.iter().rev() {
                let _ = move_file(&backup.join(m), &d.config.join(m));
            }
            let _ = std::fs::remove_dir_all(backup);
            return Err(e.context("removal failed and was rolled back"));
        }
        moved.push(f);
    }
    write_manifest(backup, Op::Remove, &files)?;
    Ok(files)
}

fn op_edit(
    name: &str,
    key: &str,
    req: &Request,
    d: &Dirs<'_>,
    backup: &Path,
) -> Result<Vec<String>> {
    let host = req.peer_host.as_deref().context("edit needs peer_host")?;
    let backups = req.backup_peer_hosts.clone().unwrap_or_default();
    let (cfg, state) = edited(d.config, name, host, &backups)?;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(backup)
        .with_context(|| format!("backup for key {key} already exists or cannot be created"))?;
    let config_name = format!("{name}.json");
    let mut files = vec![config_name.clone()];
    if state.is_some() {
        files.push(crate::setup::STATE_FILE.to_string());
    }
    for f in &files {
        let src = d.config.join(f);
        let meta = regular(&src)?.context("file disappeared")?;
        write_like(&backup.join(f), &read_bounded(&src)?, &meta)?;
    }
    write_manifest(backup, Op::Edit, &files)?;
    let cfg_path = d.config.join(&config_name);
    let cfg_meta = regular(&cfg_path)?.context("configuration disappeared")?;
    replace(&cfg_path, &cfg, &cfg_meta)?;
    if let Some(st) = state {
        let p = d.config.join(crate::setup::STATE_FILE);
        let meta = regular(&p)?.context("pairing state disappeared")?;
        if let Err(e) = replace(&p, &st, &meta) {
            let _ = replace(
                &cfg_path,
                &read_bounded(&backup.join(&config_name))?,
                &cfg_meta,
            );
            return Err(e.context("edit failed and the configuration was restored"));
        }
    }
    Ok(files)
}

fn op_revert(name: &str, d: &Dirs<'_>, backup: &Path) -> Result<Vec<String>> {
    let manifest: Value = serde_json::from_slice(&read_bounded(&backup.join(MANIFEST))?)
        .context("no backup for this key")?;
    let files: Vec<String> = manifest["files"]
        .as_array()
        .context("manifest has no files")?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let config_name = format!("{name}.json");
    // Only names this helper itself writes; anything else in a manifest is refused.
    if files.is_empty()
        || files
            .iter()
            .any(|f| f != &config_name && !PAIRING_FILES.contains(&f.as_str()))
    {
        bail!("backup manifest names unexpected files");
    }
    match manifest["op"].as_str() {
        Some("remove") => {
            for f in &files {
                if std::fs::symlink_metadata(d.config.join(f)).is_ok() {
                    bail!("{f} exists again; refusing to overwrite it");
                }
            }
            for f in &files {
                move_file(&backup.join(f), &d.config.join(f))?;
            }
            let _ = std::fs::remove_dir_all(backup);
        }
        Some("edit") => {
            for f in &files {
                let target = d.config.join(f);
                let meta = regular(&target)?.with_context(|| format!("{f} is missing"))?;
                replace(&target, &read_bounded(&backup.join(f))?, &meta)?;
            }
        }
        _ => bail!("backup manifest has an unknown operation"),
    }
    Ok(files)
}

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
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)?;
    file.write_all(receipt.to_string().as_bytes())?;
    file.sync_all()?;
    if is_root() {
        if let Some((_, gid)) = crate::setup::service_ids() {
            std::os::unix::fs::fchown(&file, None, Some(gid))?;
        }
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o640))?;
    drop(file);
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// One helper run for `name` (the fixed command of `baft-tunnel-control-manage@NAME.service`).
pub fn run(name: &str, d: &Dirs<'_>) -> Result<Value> {
    if !crate::apply::valid_instance(name) {
        bail!("invalid instance name");
    }
    let request_path = d.control.join(format!("{name}.{REQUEST_SUFFIX}"));
    let bytes = read_bounded(&request_path);
    // Consumed whatever happens, so a bad file cannot re-trigger the unit.
    let _ = std::fs::remove_file(&request_path);
    let parsed = bytes.and_then(|b| {
        let r: Request = serde_json::from_slice(&b).context("the manage request is not valid")?;
        if !crate::apply::valid_key(&r.key) {
            bail!("invalid idempotency key");
        }
        if r.op != Op::Edit && (r.peer_host.is_some() || r.backup_peer_hosts.is_some()) {
            bail!("only edit takes peer addresses");
        }
        Ok(r)
    });
    let receipt = match parsed {
        Err(e) => json!({"receipt_version": 1, "id": null, "result": "refused",
            "reason": format!("{e:#}"), "files": [], "source": "root tunnel helper"}),
        Ok(req) => {
            let instance_dir = d.backups.join(name);
            let backup = instance_dir.join(&req.key);
            let out = private_dir(d.backups)
                .and_then(|()| private_dir(&instance_dir))
                .and_then(|()| match req.op {
                    Op::Remove => op_remove(name, &req.key, d, &backup),
                    Op::Edit => op_edit(name, &req.key, &req, d, &backup),
                    Op::Revert => op_revert(name, d, &backup),
                });
            match out {
                Ok(files) => {
                    if req.op != Op::Revert {
                        prune(&instance_dir, &backup);
                    }
                    json!({"receipt_version": 1, "id": req.key, "op": req.op.as_str(),
                        "result": "applied", "files": files,
                        "backup": if req.op == Op::Revert { Value::Null } else { json!(backup.to_string_lossy()) },
                        "source": "root tunnel helper"})
                }
                Err(e) => json!({"receipt_version": 1, "id": req.key, "op": req.op.as_str(),
                    "result": "refused", "reason": format!("{e:#}"), "files": [],
                    "source": "root tunnel helper"}),
            }
        }
    };
    let mut receipt = receipt;
    receipt["secrets"] =
        json!("none recorded: file names only; key material and file contents never leave root");
    write_result(d.control, name, &receipt)?;
    Ok(receipt)
}

/// Agent side: how a manage request reaches root. Tests use a fake that runs in-process.
pub trait ManageHelper {
    fn submit(&self, name: &str, request: &Request) -> Result<Value, String>;
}

pub struct SystemManageHelper {
    pub control_dir: PathBuf,
}

impl Default for SystemManageHelper {
    fn default() -> Self {
        Self {
            control_dir: PathBuf::from(crate::apply::CONTROL_DIR),
        }
    }
}

impl ManageHelper for SystemManageHelper {
    fn submit(&self, name: &str, request: &Request) -> Result<Value, String> {
        if !crate::apply::valid_instance(name) {
            return Err("invalid instance".into());
        }
        let unit = Path::new("/etc/systemd/system/baft-tunnel-control-manage@.service");
        if !self.control_dir.is_dir() || !unit.is_file() {
            return Err(format!(
                "the root tunnel helper is not installed; run sudo scripts/install-agent-control.sh --instance {name}"
            ));
        }
        submit_in(&self.control_dir, name, request, WAIT)
    }
}

/// Write the request and wait for the result in `control_dir`.
pub fn submit_in(
    control_dir: &Path,
    name: &str,
    request: &Request,
    wait: Duration,
) -> Result<Value, String> {
    let req_path = control_dir.join(format!("{name}.{REQUEST_SUFFIX}"));
    let res_path = control_dir.join(format!("{name}.{RESULT_SUFFIX}"));
    let _ = std::fs::remove_file(&res_path);
    let body = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o660)
        .open(&req_path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                "another tunnel request for this instance is still pending".to_string()
            } else {
                format!("cannot write the tunnel request: {e}")
            }
        })?;
    file.write_all(&body)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("cannot write the tunnel request: {e}"))?;
    drop(file);
    let started = Instant::now();
    while started.elapsed() < wait {
        std::thread::sleep(Duration::from_millis(200));
        if std::fs::symlink_metadata(&res_path).is_err() {
            continue;
        }
        let bytes = read_bounded(&res_path).map_err(|e| format!("{e:#}"))?;
        let _ = std::fs::remove_file(&res_path);
        return serde_json::from_slice(&bytes)
            .map_err(|e| format!("the tunnel helper's result is not valid JSON: {e}"));
    }
    let _ = std::fs::remove_file(&req_path);
    Err(format!(
        "the root tunnel helper did not answer within {} s; check baft-tunnel-control-manage@{name}.path",
        wait.as_secs()
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) struct Env {
        pub root: PathBuf,
        pub control: PathBuf,
        pub config: PathBuf,
        pub backups: PathBuf,
        pub run: PathBuf,
    }

    impl Drop for Env {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    impl Env {
        pub(crate) fn new(tag: &str) -> Self {
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let root =
                std::env::temp_dir().join(format!("baft-manage-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let e = Self {
                control: root.join("control"),
                config: root.join("etc"),
                backups: root.join("backups"),
                run: root.join("run"),
                root,
            };
            for p in [&e.control, &e.config, &e.run] {
                std::fs::create_dir_all(p).unwrap();
            }
            e
        }
        pub(crate) fn dirs(&self) -> Dirs<'_> {
            Dirs {
                control: &self.control,
                config: &self.config,
                backups: &self.backups,
                run: &self.run,
            }
        }
        /// A forward instance created by setup (config, pairing state, bundle, key) plus PKI files.
        pub(crate) fn forward(&self, name: &str) {
            let cfg = json!({"mode": "forward", "engine": "noise-tcp", "direction": "direct",
                "listen": "127.0.0.1:18080", "remote": "203.0.113.7:8443", "local_id": "iran",
                "peer_id": "abroad", "tunnel_id": "demo", "key": "/etc/baft-tunnel/node.noise",
                "noise_peer_public": "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA="});
            std::fs::write(self.config.join(format!("{name}.json")), cfg.to_string()).unwrap();
            std::fs::write(
                self.config.join("pairing-state.json"),
                json!({"config_file": format!("{name}.json"), "peer_host": "203.0.113.7"})
                    .to_string(),
            )
            .unwrap();
            std::fs::write(self.config.join("bundle.json"), "{}").unwrap();
            std::fs::write(self.config.join("node.noise"), "secret-key-bytes").unwrap();
            std::fs::set_permissions(
                self.config.join("node.noise"),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
            std::fs::write(self.config.join("ca.crt"), "operator pki").unwrap();
        }
        pub(crate) fn put(&self, name: &str, req: &Value) {
            std::fs::write(self.control.join(format!("{name}.manage")), req.to_string()).unwrap();
        }
    }

    #[test]
    fn remove_moves_owned_files_to_a_private_backup_and_revert_restores_them() {
        let e = Env::new("rm");
        e.forward("edge");
        e.put("edge", &json!({"op": "remove", "key": "rm-key-0001"}));
        let out = run("edge", &e.dirs()).unwrap();
        assert_eq!(out["result"], "applied", "{out}");
        assert_eq!(
            out["files"],
            json!([
                "edge.json",
                "pairing-state.json",
                "bundle.json",
                "node.noise"
            ])
        );
        assert!(!e.config.join("edge.json").exists());
        assert!(!e.config.join("node.noise").exists());
        assert!(e.config.join("ca.crt").exists(), "operator PKI is kept");
        let backup = e.backups.join("edge/rm-key-0001");
        let mode = std::fs::metadata(&backup).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        let kmode = std::fs::metadata(backup.join("node.noise"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(kmode & 0o777, 0o600);
        let text = out.to_string();
        assert!(!text.contains("secret-key-bytes"), "{text}");
        assert!(!e.control.join("edge.manage").exists(), "request consumed");

        e.put("edge", &json!({"op": "revert", "key": "rm-key-0001"}));
        let out = run("edge", &e.dirs()).unwrap();
        assert_eq!(out["result"], "applied", "{out}");
        assert_eq!(
            std::fs::read_to_string(e.config.join("node.noise")).unwrap(),
            "secret-key-bytes"
        );
        assert!(e.config.join("edge.json").exists());
        assert!(!backup.exists());
    }

    #[test]
    fn remove_refuses_a_running_unit_and_leaves_shared_pairing_files_of_another_instance() {
        let e = Env::new("rm-run");
        e.forward("other");
        std::fs::write(e.config.join("edge.json"), "{}").unwrap();
        std::fs::create_dir(e.run.join("edge")).unwrap();
        e.put("edge", &json!({"op": "remove", "key": "rm-key-0002"}));
        let out = run("edge", &e.dirs()).unwrap();
        assert_eq!(out["result"], "refused");
        assert!(out["reason"].as_str().unwrap().contains("still running"));
        assert!(e.config.join("edge.json").exists());
        std::fs::remove_dir(e.run.join("edge")).unwrap();
        e.put("edge", &json!({"op": "remove", "key": "rm-key-0003"}));
        let out = run("edge", &e.dirs()).unwrap();
        assert_eq!(out["result"], "applied", "{out}");
        assert_eq!(out["files"], json!(["edge.json"]));
        assert!(e.config.join("node.noise").exists(), "belongs to 'other'");
        assert!(e.config.join("other.json").exists());
    }

    #[test]
    fn remove_refuses_links_missing_config_and_reused_keys() {
        let e = Env::new("rm-bad");
        e.put("edge", &json!({"op": "remove", "key": "rm-key-0004"}));
        assert_eq!(run("edge", &e.dirs()).unwrap()["result"], "refused");
        let victim = e.root.join("victim.json");
        std::fs::write(&victim, "{}").unwrap();
        std::os::unix::fs::symlink(&victim, e.config.join("edge.json")).unwrap();
        e.put("edge", &json!({"op": "remove", "key": "rm-key-0005"}));
        let out = run("edge", &e.dirs()).unwrap();
        assert_eq!(out["result"], "refused", "{out}");
        assert!(victim.exists());
        std::fs::remove_file(e.config.join("edge.json")).unwrap();
        e.forward("edge");
        e.put(
            "edge",
            &json!({"op": "edit", "key": "rm-key-0006", "peer_host": "198.51.100.9"}),
        );
        assert_eq!(run("edge", &e.dirs()).unwrap()["result"], "applied");
        e.put("edge", &json!({"op": "remove", "key": "rm-key-0006"}));
        let out = run("edge", &e.dirs()).unwrap();
        assert_eq!(out["result"], "refused", "same key, existing backup");
        assert!(e.config.join("edge.json").exists());
    }

    #[test]
    fn edit_changes_only_peer_addresses_keeps_the_port_and_reverts() {
        let e = Env::new("edit");
        e.forward("edge");
        let before = std::fs::read(e.config.join("edge.json")).unwrap();
        e.put(
            "edge",
            &json!({"op": "edit", "key": "ed-key-0001", "peer_host": "198.51.100.9",
                "backup_peer_hosts": ["198.51.100.10"]}),
        );
        let out = run("edge", &e.dirs()).unwrap();
        assert_eq!(out["result"], "applied", "{out}");
        let cfg: Value =
            serde_json::from_slice(&std::fs::read(e.config.join("edge.json")).unwrap()).unwrap();
        assert_eq!(cfg["remote"], "198.51.100.9:8443");
        assert_eq!(cfg["backup_remotes"], json!(["198.51.100.10:8443"]));
        assert_eq!(cfg["tunnel_id"], "demo");
        let st: Value =
            serde_json::from_slice(&std::fs::read(e.config.join("pairing-state.json")).unwrap())
                .unwrap();
        assert_eq!(st["peer_host"], "198.51.100.9");
        e.put("edge", &json!({"op": "revert", "key": "ed-key-0001"}));
        assert_eq!(run("edge", &e.dirs()).unwrap()["result"], "applied");
        assert_eq!(std::fs::read(e.config.join("edge.json")).unwrap(), before);
    }

    #[test]
    fn edit_refuses_bad_addresses_serve_side_unknown_fields_and_no_change() {
        let e = Env::new("edit-bad");
        e.forward("edge");
        let before = std::fs::read(e.config.join("edge.json")).unwrap();
        for (i, req) in [
            json!({"op": "edit", "key": "ed-bad-0001", "peer_host": "example.com"}),
            json!({"op": "edit", "key": "ed-bad-0002", "peer_host": "127.0.0.1"}),
            json!({"op": "edit", "key": "ed-bad-0003", "peer_host": "0.0.0.0"}),
            json!({"op": "edit", "key": "ed-bad-0004", "peer_host": "203.0.113.7"}),
            json!({"op": "edit", "key": "ed-bad-0005", "peer_host": "198.51.100.9",
                "backup_peer_hosts": ["198.51.100.9"]}),
            json!({"op": "edit", "key": "ed-bad-0006", "peer_host": "198.51.100.9",
                "backup_peer_hosts": ["198.51.100.1", "198.51.100.2", "198.51.100.3", "198.51.100.4"]}),
            json!({"op": "edit", "key": "ed-bad-0007", "peer_host": "198.51.100.9", "listen": "0.0.0.0:1"}),
            json!({"op": "remove", "key": "ed-bad-0008", "peer_host": "198.51.100.9"}),
            json!({"op": "edit", "key": "../../x", "peer_host": "198.51.100.9"}),
        ]
        .iter()
        .enumerate()
        {
            e.put("edge", req);
            let out = run("edge", &e.dirs()).unwrap();
            assert_eq!(out["result"], "refused", "case {i}: {out}");
        }
        assert_eq!(std::fs::read(e.config.join("edge.json")).unwrap(), before);
        std::fs::write(
            e.config.join("srv.json"),
            json!({"mode": "serve", "listen": "0.0.0.0:8443"}).to_string(),
        )
        .unwrap();
        e.put(
            "srv",
            &json!({"op": "edit", "key": "ed-bad-0009", "peer_host": "198.51.100.9"}),
        );
        let out = run("srv", &e.dirs()).unwrap();
        assert_eq!(out["result"], "refused");
        assert!(out["reason"].as_str().unwrap().contains("forward"), "{out}");
    }

    #[test]
    fn backups_are_pruned_to_the_newest_three() {
        let e = Env::new("prune");
        e.forward("edge");
        for (i, host) in [
            "198.51.100.1",
            "198.51.100.2",
            "198.51.100.3",
            "198.51.100.4",
            "198.51.100.5",
        ]
        .iter()
        .enumerate()
        {
            e.put(
                "edge",
                &json!({"op": "edit", "key": format!("pr-key-000{i}"), "peer_host": host}),
            );
            assert_eq!(run("edge", &e.dirs()).unwrap()["result"], "applied");
            std::thread::sleep(Duration::from_millis(20));
        }
        let n = std::fs::read_dir(e.backups.join("edge")).unwrap().count();
        assert_eq!(n, KEEP_BACKUPS);
        assert!(e.backups.join("edge/pr-key-0004").exists());
    }

    #[test]
    fn submit_in_times_out_and_cleans_its_request() {
        let e = Env::new("submit");
        let req = Request {
            op: Op::Remove,
            key: "sb-key-0001".into(),
            peer_host: None,
            backup_peer_hosts: None,
        };
        let err = submit_in(&e.control, "edge", &req, Duration::from_millis(300)).unwrap_err();
        assert!(err.contains("did not answer"));
        assert!(!e.control.join("edge.manage").exists());
    }
}
