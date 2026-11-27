//! Root tunnel management helper (ADR-0055): remove or edit the one instance an agent is bound to.
//!
//! The agent connects to the per-instance root-owned Unix socket. The socket-activated
//! `baft-tunnel-control-manage@NAME.service` authenticates kernel peer credentials and the exact
//! agent systemd cgroup before accepting one bounded JSON request and applying it:
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
//! - `policy` (ADR-0057): `enable` writes the managed drop-in that sets
//!   `BAFT_TUNNEL_POLICY_PACK=sched`, `disable` removes only that byte-exact file, `select` sets
//!   `policy.profile` and `reset` clears `policy.profile`/`policy.adaptive`. The helper reloads
//!   systemd, restarts a running unit, verifies the result through `/proc/<MainPID>/environ`, and
//!   restores both files' previous bytes when any step fails.
//!
//! The agent never sends file contents or paths. A failure part-way restores what was moved. The
//! socket receipt lists file names only, never contents.

use std::ffi::CString;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr};
#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

pub const SOCKET_DIR: &str = "/run/baft-tunnel/manage";
pub const SOCKET_SUFFIX: &str = "manage.sock";
#[cfg(test)]
const REQUEST_SUFFIX: &str = "manage";
pub const BACKUP_ROOT: &str = "/var/lib/baft-tunnel-control/tunnel-backups";
pub const RUN_ROOT: &str = "/run/baft-tunnel";
pub const UNIT_ROOT: &str = "/etc/systemd/system";
/// The runtime switch the data path reads (ADR-0031). `sched` selects the configured profile's
/// scheduler pair; the operator surface never writes `window-cap` (ADR-0057 decision 2).
pub const PACK_SWITCH: &str = "BAFT_TUNNEL_POLICY_PACK";
pub const PACK_MODE: &str = "sched";
/// The one file `enable` writes and `disable` removes, under `baft-tunnel@NAME.service.d/`.
pub const DROP_IN: &str = "50-policy-pack.conf";
/// Exactly what `enable` writes. `disable` removes the file only when it still has these bytes.
pub const MANAGED_DROP_IN: &[u8] = b"[Service]\nEnvironment=BAFT_TUNNEL_POLICY_PACK=sched\n";
const MAX_FILE_BYTES: u64 = 64 * 1024;
const MANIFEST: &str = "manifest.json";
/// Backups kept per instance; older ones are deleted after a successful operation.
const KEEP_BACKUPS: usize = 3;
/// Shorter than the root helper unit's `TimeoutStartSec=50s`, leaving time to report a timeout.
pub const WAIT: Duration = Duration::from_secs(40);
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
    Policy,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Remove => "remove",
            Self::Edit => "edit",
            Self::Revert => "revert",
            Self::Policy => "policy",
        }
    }
}

/// ADR-0057 decision 1: the closed action set of `POST /v1/policy/apply`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyAction {
    /// Write the switch drop-in (the pack's gate).
    Enable,
    /// Remove the switch drop-in; refused when the switch belongs to someone else.
    Disable,
    /// Set `policy.profile`, preserving `policy.adaptive`.
    Select,
    /// Clear `policy.profile` and `policy.adaptive`; the switch is left alone.
    Reset,
}

impl PolicyAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Select => "select",
            Self::Reset => "reset",
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
    /// `policy` only: which of the four writes to make (ADR-0057).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<PolicyAction>,
    /// `policy select` only: the versioned profile id. Refused for every other action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

pub struct Dirs<'a> {
    pub control: &'a Path,
    pub config: &'a Path,
    pub backups: &'a Path,
    pub run: &'a Path,
    /// systemd unit directory (`/etc/systemd/system`); the managed drop-in lives below it.
    pub units: &'a Path,
    /// What the helper may do to the unit after a policy write. Tests substitute a fake.
    pub service: &'a dyn ServiceControl,
}

/// The unit operations an ADR-0057 write needs, behind one trait so the tests can drive every
/// outcome (reload, restart, and the running process's environment) without systemd.
pub trait ServiceControl {
    fn reload(&self) -> Result<()>;
    /// Restart `baft-tunnel@NAME` and wait until systemd reports it active again.
    fn restart(&self, name: &str) -> Result<()>;
    /// The running unit's `BAFT_TUNNEL_POLICY_PACK` as systemd applied it (`systemctl show`).
    fn switch_value(&self, name: &str) -> Result<Option<String>>;
}

/// Production: `systemctl` and `/proc`. Reads only; the write path is the files above.
pub struct SystemService;

impl SystemService {
    fn systemctl(args: &[&str]) -> Result<std::process::Output> {
        std::process::Command::new("systemctl")
            .args(args)
            .output()
            .with_context(|| format!("run systemctl {}", args.join(" ")))
    }
}

impl ServiceControl for SystemService {
    fn reload(&self) -> Result<()> {
        let out = Self::systemctl(&["daemon-reload"])?;
        if !out.status.success() {
            bail!(
                "systemctl daemon-reload failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    fn restart(&self, name: &str) -> Result<()> {
        let unit = format!("baft-tunnel@{name}");
        let out = Self::systemctl(&["restart", &unit])?;
        if !out.status.success() {
            bail!(
                "systemctl restart {unit} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        for _ in 0..40 {
            let state = Self::systemctl(&["is-active", &unit])?;
            if String::from_utf8_lossy(&state.stdout).trim() == "active" {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        bail!("{unit} did not become active after the restart")
    }

    fn switch_value(&self, name: &str) -> Result<Option<String>> {
        let unit = format!("baft-tunnel@{name}");
        // `Environment` is the environment systemd itself applied to this unit's processes. It is
        // readable by the helper without CAP_SYS_PTRACE, which `/proc/<MainPID>/environ` of a
        // `User=` unit would need (the installed service does not have that capability).
        let out = Self::systemctl(&["show", "-p", "Environment", "--value", &unit])?;
        if !out.status.success() {
            bail!(
                "systemctl show {unit} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(pack_switch_in_environment(&String::from_utf8_lossy(
            &out.stdout,
        )))
    }
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
/// New bytes for the config (and pairing state) plus the exact bytes they were derived from.
struct Edited {
    cfg: Vec<u8>,
    cfg_old: Vec<u8>,
    state: Option<(Vec<u8>, Vec<u8>)>,
}

fn edited(config_dir: &Path, name: &str, peer_host: &str, backups: &[String]) -> Result<Edited> {
    let config_path = config_dir.join(format!("{name}.json"));
    let cfg_old = read_bounded(&config_path)?;
    let mut cfg: Value =
        serde_json::from_slice(&cfg_old).context("the current configuration is not valid JSON")?;
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
        let st_old = read_bounded(&state_path)?;
        let mut st: Value = serde_json::from_slice(&st_old).unwrap_or(Value::Null);
        if st["config_file"].as_str() == Some(format!("{name}.json").as_str()) {
            st["peer_host"] = json!(primary.to_string());
            st["backup_peer_hosts"] = json!(
                seen[1..]
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            );
            Some((serde_json::to_vec_pretty(&st)?, st_old))
        } else {
            None
        }
    } else {
        None
    };
    Ok(Edited {
        cfg: serde_json::to_vec_pretty(&cfg)?,
        cfg_old,
        state,
    })
}

fn sha_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}

/// `written`: for an edit, the sha256 of each file as the edit wrote it; a revert only puts the
/// old bytes back while the current file still has exactly that content (compare-and-swap).
fn write_manifest(dir: &Path, op: Op, files: &[String], written: &[String]) -> Result<()> {
    let body =
        serde_json::to_vec(&json!({"op": op.as_str(), "files": files, "written_sha256": written}))?;
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

/// Delete all but the newest `KEEP_BACKUPS` edit backups of this instance (never `keep`).
fn prune(instance_dir: &Path, keep: &Path) {
    let Ok(rd) = std::fs::read_dir(instance_dir) else {
        return;
    };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let m = std::fs::symlink_metadata(e.path()).ok()?;
            let t = m.modified().ok()?;
            // Remove backups hold the only copy of a removed key: never deleted automatically.
            let man: Value =
                serde_json::from_slice(&read_bounded(&e.path().join(MANIFEST)).ok()?).ok()?;
            let kept = matches!(man["op"].as_str(), Some("edit" | "policy"));
            (m.is_dir() && e.path() != keep && kept).then(|| (t, e.path()))
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
            let mut lost = Vec::new();
            for m in moved.iter().rev() {
                if move_file(&backup.join(m), &d.config.join(m)).is_err() {
                    lost.push(m.as_str());
                }
            }
            if lost.is_empty() {
                let _ = std::fs::remove_dir_all(backup);
                return Err(e.context("removal failed and was rolled back"));
            }
            // Never delete the only copy: keep the backup and say what is still in it.
            let _ = write_manifest(backup, Op::Remove, &files, &[]);
            return Err(e.context(format!(
                "removal failed; {} could not be put back and stay in {}",
                lost.join(", "),
                backup.display()
            )));
        }
        moved.push(f);
    }
    write_manifest(backup, Op::Remove, &files, &[])?;
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
    let Edited {
        cfg,
        cfg_old,
        state,
    } = edited(d.config, name, host, &backups)?;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(backup)
        .with_context(|| format!("backup for key {key} already exists or cannot be created"))?;
    let config_name = format!("{name}.json");
    let mut files = vec![config_name.clone()];
    if state.is_some() {
        files.push(crate::setup::STATE_FILE.to_string());
    }
    let mut olds = vec![cfg_old.as_slice()];
    let mut written = vec![sha_hex(&cfg)];
    if let Some((new, old)) = &state {
        olds.push(old);
        written.push(sha_hex(new));
    }
    for (f, old) in files.iter().zip(&olds) {
        let src = d.config.join(f);
        let meta = regular(&src)?.context("file disappeared")?;
        // The exact bytes the edit was derived from; refuse if the file changed meanwhile.
        if read_bounded(&src)? != *old {
            let _ = std::fs::remove_dir_all(backup);
            bail!("{f} changed during the edit; nothing was changed");
        }
        write_like(&backup.join(f), old, &meta)?;
    }
    write_manifest(backup, Op::Edit, &files, &written)?;
    let cfg_path = d.config.join(&config_name);
    let cfg_meta = regular(&cfg_path)?.context("configuration disappeared")?;
    replace(&cfg_path, &cfg, &cfg_meta)?;
    if let Some((st, _)) = state {
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

/// The policy object of a configuration, as this surface reads and writes it.
fn policy_row(cfg: &Value) -> Result<(Option<String>, bool)> {
    match cfg.get("policy") {
        None | Some(Value::Null) => Ok((None, false)),
        Some(Value::Object(map)) => Ok((
            map.get("profile")
                .and_then(Value::as_str)
                .map(str::to_string),
            map.get("adaptive")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )),
        Some(_) => bail!("policy in the configuration is not an object"),
    }
}

fn drop_in_dir(d: &Dirs<'_>, name: &str) -> PathBuf {
    d.units.join(format!("baft-tunnel@{name}.service.d"))
}

/// A profile id is a runtime-validated versioned name (`balanced/1`): shape only here, so the
/// accepted list stays in one place — the runtime's own profile table.
fn plausible_profile_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
}

/// The switch inside `systemctl show -p Environment` output (`A=1 B=2`, where a value may itself be
/// quoted and contain spaces). Only our own variable is looked for.
fn pack_switch_in_environment(text: &str) -> Option<String> {
    let prefix = format!("{PACK_SWITCH}=");
    let at = text.find(&prefix)?;
    let after = &text[at + prefix.len()..];
    let value = match after.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next().unwrap_or_default(),
        None => after.split_whitespace().next().unwrap_or_default(),
    };
    (!value.is_empty()).then(|| value.to_string())
}

fn mentions_switch(bytes: &[u8]) -> bool {
    let prefix = format!("{PACK_SWITCH}=");
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .any(|line| !line.starts_with('#') && line.contains(&prefix))
}

/// A unit file or another drop-in that already sets the switch: this surface leaves it alone.
fn switch_elsewhere(d: &Dirs<'_>, name: &str) -> Result<Option<String>> {
    for unit in [
        d.units.join(format!("baft-tunnel@{name}.service")),
        // The template applies to every instance, so a switch set there is just as much someone
        // else's configuration as one in this instance's own unit file.
        d.units.join("baft-tunnel@.service"),
    ] {
        if regular(&unit)?.is_some() && mentions_switch(&read_bounded(&unit)?) {
            return Ok(Some(unit.to_string_lossy().into_owned()));
        }
    }
    // Drop-ins for this instance and for the template, each searched for `.conf` files only.
    for dir in [drop_in_dir(d, name), d.units.join("baft-tunnel@.service.d")] {
        if let Some(path) = switch_in_dir(&dir)? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// A `.conf` drop-in in `dir` that sets the switch, other than this surface's own file.
fn switch_in_dir(dir: &Path) -> Result<Option<String>> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(None);
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        let is_conf = path.extension().and_then(|e| e.to_str()) == Some("conf");
        let is_managed = path.file_name().and_then(|n| n.to_str()) == Some(DROP_IN);
        if !is_conf || is_managed {
            continue;
        }
        if regular(&path)?.is_some() && mentions_switch(&read_bounded(&path)?) {
            return Ok(Some(path.to_string_lossy().into_owned()));
        }
    }
    Ok(None)
}

/// Create or replace a file with exactly `mode`, root-owned (the helper runs as root).
fn write_fixed(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let dir = path.parent().context("the file has no directory")?;
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
        Ok(_) => bail!("{} is not a real directory", dir.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o755)
                .create(dir)
                .with_context(|| format!("create {}", dir.display()))?;
        }
        Err(e) => return Err(e).with_context(|| format!("inspect {}", dir.display())),
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("bad file name")?;
    let tmp = path.with_file_name(format!(".{name}.manage.tmp"));
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)
        .with_context(|| format!("create {}", tmp.display()))?;
    f.write_all(bytes)?;
    f.set_permissions(std::fs::Permissions::from_mode(mode))?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}

enum PolicyOutcome {
    /// Nothing was written because the request itself is refused (an operator-owned file, an
    /// invalid id, an engine out of scope): the reason names exactly what was not changed.
    Refused { cause: &'static str, reason: String },
    /// A complete receipt: `configured`, `unchanged`, `rolled_back` or `recovery_failed`.
    Done(Value),
}

enum DropInChange {
    Keep,
    Write,
    Remove,
}

fn next_commands(name: &str, action: PolicyAction, profile: Option<&str>) -> Value {
    let retry = match (action, profile) {
        (PolicyAction::Select, Some(id)) => {
            format!("baft-tunnel policy select {id} --instance {name}")
        }
        _ => format!("baft-tunnel policy {} --instance {name}", action.as_str()),
    };
    json!([
        format!("systemctl status baft-tunnel@{name}"),
        retry,
        "re-running the same command converges to \"unchanged\" without writing".to_string(),
    ])
}

#[allow(clippy::too_many_arguments)]
fn policy_receipt(
    key: &str,
    name: &str,
    action: PolicyAction,
    result: &str,
    previous: Value,
    requested: Value,
    files: Vec<String>,
    restart_required: bool,
    verification: Value,
    backup: Option<&Path>,
    profile: Option<&str>,
) -> Value {
    json!({
        "receipt_version": 1, "id": key, "op": "policy", "action": action.as_str(),
        "result": result, "instance": name, "previous": previous, "requested": requested,
        "files": files, "restart_required": restart_required, "verification": verification,
        "backup": backup.map(|p| json!(p.to_string_lossy())).unwrap_or(Value::Null),
        "next_commands": next_commands(name, action, profile),
        "source": "root tunnel helper",
        "secrets": "none recorded: file names only; key material and file contents never leave root",
    })
}

/// One ADR-0057 write. Validates with the runtime's own schema, writes with a per-key backup,
/// reloads systemd, restarts a running unit, verifies through files and the unit's own environment,
/// and restores the files this request replaced when any step fails (ADR-0057 decision 4).
fn apply_policy(name: &str, req: &Request, d: &Dirs<'_>) -> Result<PolicyOutcome> {
    let action = req.action.context("policy needs an action")?;
    let config_name = format!("{name}.json");
    let config_path = d.config.join(&config_name);
    let cfg_old = read_bounded(&config_path)?;
    let cfg_meta = regular(&config_path)?.context("configuration disappeared")?;
    let cfg: Value =
        serde_json::from_slice(&cfg_old).context("the current configuration is not valid JSON")?;
    // The configuration on disk is validated before anything is written: an operator may have
    // introduced an unknown id after the agent loaded its copy, and restarting the unit on it
    // before finding out would be a write this request did not need to make.
    if let Err(error) = validate_config(&cfg) {
        return Ok(PolicyOutcome::Refused {
            cause: "policy_not_valid",
            reason: format!(
                "the configuration on disk is already invalid, so nothing was changed: {error:#}"
            ),
        });
    }
    let (profile_before, adaptive_before) = policy_row(&cfg)?;
    let drop_path = drop_in_dir(d, name).join(DROP_IN);
    let drop_old = match regular(&drop_path)? {
        Some(_) => Some(read_bounded(&drop_path)?),
        None => None,
    };
    let managed = drop_old.as_deref() == Some(MANAGED_DROP_IN);
    let elsewhere = switch_elsewhere(d, name)?;
    let mut previous = json!({
        "switch": if managed { json!(PACK_MODE) } else { Value::Null },
        "profile": profile_before, "adaptive": adaptive_before,
    });
    if let Some(path) = &elsewhere {
        previous["switch_managed_elsewhere"] = json!(path);
    }
    let mut requested = previous.clone();
    let new_cfg: Option<Vec<u8>> = match action {
        PolicyAction::Enable => {
            if let Some(path) = &elsewhere {
                return Ok(PolicyOutcome::Refused {
                    cause: "switch_managed_elsewhere",
                    reason: format!(
                        "the switch is set in {path}; this surface only manages {} and never edits operator-owned unit configuration",
                        drop_path.display()
                    ),
                });
            }
            if drop_old.is_some() && !managed {
                return Ok(PolicyOutcome::Refused {
                    cause: "switch_managed_elsewhere",
                    reason: format!(
                        "{} exists with content this surface did not write; it was left untouched",
                        drop_path.display()
                    ),
                });
            }
            requested["switch"] = json!(PACK_MODE);
            if managed {
                return Ok(PolicyOutcome::Done(policy_receipt(
                    &req.key,
                    name,
                    action,
                    "unchanged",
                    previous,
                    requested,
                    vec![],
                    false,
                    json!({"level": "file", "reason": "the managed drop-in already holds exactly these bytes; nothing was written and the unit was not restarted"}),
                    None,
                    None,
                )));
            }
            None
        }
        PolicyAction::Disable => {
            requested["switch"] = Value::Null;
            // A second file that also sets the switch would keep it set after this file is gone,
            // so that is a refusal whether or not the managed file exists.
            if let Some(path) = &elsewhere {
                return Ok(PolicyOutcome::Refused {
                    cause: "switch_managed_elsewhere",
                    reason: format!(
                        "the switch is also set in {path}; removing {} would leave {PACK_SWITCH} in effect, so nothing was changed",
                        drop_path.display()
                    ),
                });
            }
            if !managed {
                if drop_old.is_some() {
                    return Ok(PolicyOutcome::Refused {
                        cause: "switch_not_managed",
                        reason: format!(
                            "{} was not written by this surface; remove it yourself if it is yours",
                            drop_path.display()
                        ),
                    });
                }
                return Ok(PolicyOutcome::Done(policy_receipt(
                    &req.key,
                    name,
                    action,
                    "unchanged",
                    previous,
                    requested,
                    vec![],
                    false,
                    json!({"level": "file", "reason": "the switch is not set anywhere this surface can see; nothing was written"}),
                    None,
                    None,
                )));
            }
            None
        }
        PolicyAction::Select => {
            let id = req.profile.as_deref().context("select needs a profile")?;
            requested["profile"] = json!(id);
            if profile_before.as_deref() == Some(id) {
                return Ok(PolicyOutcome::Done(policy_receipt(
                    &req.key,
                    name,
                    action,
                    "unchanged",
                    previous,
                    requested,
                    vec![],
                    false,
                    json!({"level": "file", "reason": "policy.profile already holds this id; nothing was written and the unit was not restarted"}),
                    None,
                    Some(id),
                )));
            }
            let mut next = cfg.clone();
            let policy = next
                .as_object_mut()
                .context("the configuration is not an object")?
                .entry("policy")
                .or_insert_with(|| json!({}));
            policy
                .as_object_mut()
                .context("policy in the configuration is not an object")?
                .insert("profile".into(), json!(id));
            let bytes = match validated_config(&next) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return Ok(PolicyOutcome::Refused {
                        cause: "policy_not_valid",
                        reason: format!("{error:#}"),
                    });
                }
            };
            Some(bytes)
        }
        PolicyAction::Reset => {
            requested["profile"] = Value::Null;
            requested["adaptive"] = json!(false);
            if profile_before.is_none() && !adaptive_before {
                return Ok(PolicyOutcome::Done(policy_receipt(
                    &req.key,
                    name,
                    action,
                    "unchanged",
                    previous,
                    requested,
                    vec![],
                    false,
                    json!({"level": "file", "reason": "policy.profile and policy.adaptive are already absent; nothing was written and the unit was not restarted"}),
                    None,
                    None,
                )));
            }
            let mut next = cfg.clone();
            let mut empty = false;
            if let Some(policy) = next.get_mut("policy").and_then(Value::as_object_mut) {
                policy.remove("profile");
                policy.remove("adaptive");
                empty = policy.is_empty();
            }
            if empty {
                next.as_object_mut()
                    .context("the configuration is not an object")?
                    .remove("policy");
            }
            let bytes = match validated_config(&next) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return Ok(PolicyOutcome::Refused {
                        cause: "policy_not_valid",
                        reason: format!("{error:#}"),
                    });
                }
            };
            Some(bytes)
        }
    };
    let (change, touched_name) = match action {
        PolicyAction::Enable => (DropInChange::Write, DROP_IN),
        PolicyAction::Disable => (DropInChange::Remove, DROP_IN),
        _ => (DropInChange::Keep, ""),
    };
    let was_up = std::fs::symlink_metadata(d.run.join(name)).is_ok();
    // The running unit's switch before the write: recovery compares against this exact value, not
    // against whatever the files imply, and says so when it cannot be read.
    let baseline = if was_up {
        match d.service.switch_value(name) {
            Ok(value) => SwitchBaseline::Known(value),
            Err(error) => SwitchBaseline::Unreadable(format!("{error:#}")),
        }
    } else {
        SwitchBaseline::Stopped
    };
    // Per-key backup of the exact bytes this request may replace (the Op::Edit mechanics).
    private_dir(d.backups)?;
    let instance_dir = d.backups.join(name);
    private_dir(&instance_dir)?;
    let backup = instance_dir.join(&req.key);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&backup)
        .with_context(|| {
            format!(
                "backup for key {} already exists or cannot be created",
                req.key
            )
        })?;
    if let Err(error) = write_like(&backup.join(&config_name), &cfg_old, &cfg_meta) {
        let _ = std::fs::remove_dir_all(&backup);
        return Err(
            error.context("the previous configuration could not be backed up; nothing was changed")
        );
    }
    if let Some(bytes) = &drop_old {
        write_fixed(&backup.join(DROP_IN), bytes, 0o644)?;
    }
    // The manifest describes the state this request is responsible for: both files' previous bytes
    // are on disk, and the hashes are what a later revert may expect to find.
    let mut files = vec![config_name.clone()];
    let mut hashes = vec![sha_hex(new_cfg.as_deref().unwrap_or(&cfg_old))];
    match change {
        DropInChange::Write => {
            files.push(DROP_IN.to_string());
            hashes.push(sha_hex(MANAGED_DROP_IN));
        }
        DropInChange::Keep => {
            if let Some(bytes) = &drop_old {
                files.push(DROP_IN.to_string());
                hashes.push(sha_hex(bytes));
            }
        }
        DropInChange::Remove => {}
    }
    write_manifest(&backup, Op::Policy, &files, &hashes)?;
    let policy_files = PolicyFiles {
        config_name: config_name.clone(),
        config_path: config_path.clone(),
        cfg_old: cfg_old.clone(),
        cfg_meta: cfg_meta.clone(),
        drop_path: drop_path.clone(),
        drop_old: drop_old.clone(),
        was_up,
        baseline,
        backup: backup.clone(),
    };
    let mut written: Vec<Written> = Vec::new();
    let mutation = (|| -> Result<()> {
        if let Some(bytes) = &new_cfg {
            replace(&config_path, bytes, &cfg_meta)?;
            written.push(Written::replaced(
                config_name.clone(),
                config_path.clone(),
                bytes,
            ));
        }
        match change {
            DropInChange::Write => {
                write_fixed(&drop_path, MANAGED_DROP_IN, 0o644)?;
                written.push(Written::replaced(
                    DROP_IN.to_string(),
                    drop_path.clone(),
                    MANAGED_DROP_IN,
                ));
            }
            DropInChange::Remove => {
                std::fs::remove_file(&drop_path)
                    .with_context(|| format!("remove {}", drop_path.display()))?;
                written.push(Written::removed(DROP_IN.to_string(), drop_path.clone()));
            }
            DropInChange::Keep => {}
        }
        Ok(())
    })();
    if let Err(error) = mutation {
        return Ok(recover_policy(
            name,
            req,
            d,
            action,
            &previous,
            &requested,
            &policy_files,
            &written,
            format!("write: {error:#}"),
        ));
    }
    let check = verify_policy(
        action,
        name,
        d,
        &policy_files,
        new_cfg.as_deref(),
        req.profile.as_deref(),
    );
    if let Err(error) = check {
        return Ok(recover_policy(
            name,
            req,
            d,
            action,
            &previous,
            &requested,
            &policy_files,
            &written,
            format!("verify: {error:#}"),
        ));
    }
    let mut touched = vec![config_name];
    if !touched_name.is_empty() {
        touched.push(touched_name.to_string());
    }
    Ok(PolicyOutcome::Done(policy_receipt(
        &req.key,
        name,
        action,
        "configured",
        previous,
        requested,
        touched,
        !was_up,
        json!({
            "level": if was_up { "unit" } else { "file" },
            "reason": if was_up {
                "systemd was reloaded, the unit was restarted and became active, and its own environment was re-read"
            } else {
                "the unit was stopped before this request and was not started; only the files were verified"
            },
        }),
        Some(&backup),
        req.profile.as_deref(),
    )))
}

/// The state of one instance's files and unit that a single policy request works against.
struct PolicyFiles {
    config_name: String,
    config_path: PathBuf,
    cfg_old: Vec<u8>,
    cfg_meta: std::fs::Metadata,
    drop_path: PathBuf,
    drop_old: Option<Vec<u8>>,
    was_up: bool,
    baseline: SwitchBaseline,
    backup: PathBuf,
}

impl PolicyFiles {
    /// The bytes a label held before this request (the drop-in: `None` when it was absent).
    fn previous(&self, label: &str) -> Option<&[u8]> {
        if label == self.config_name {
            Some(&self.cfg_old)
        } else {
            self.drop_old.as_deref()
        }
    }

    fn path(&self, label: &str) -> &Path {
        if label == self.config_name {
            &self.config_path
        } else {
            &self.drop_path
        }
    }
}

/// What the running unit had before the write: recovery restores the files and then compares the
/// unit against this, rather than assuming.
enum SwitchBaseline {
    Stopped,
    Known(Option<String>),
    Unreadable(String),
}

/// One file this request actually replaced or removed, with the bytes it left there, so recovery
/// reverts only its own writes and never overwrites an edit made while it ran.
enum Written {
    Replaced {
        label: String,
        path: PathBuf,
        bytes: Vec<u8>,
    },
    Removed {
        label: String,
        path: PathBuf,
    },
}

impl Written {
    fn replaced(label: String, path: PathBuf, bytes: &[u8]) -> Self {
        Self::Replaced {
            label,
            path,
            bytes: bytes.to_vec(),
        }
    }

    fn removed(label: String, path: PathBuf) -> Self {
        Self::Removed { label, path }
    }

    fn label(&self) -> &str {
        match self {
            Self::Replaced { label, .. } | Self::Removed { label, .. } => label,
        }
    }

    /// The file still holds exactly what this request left there.
    fn holds_its_own_write(&self) -> bool {
        match self {
            Self::Replaced { path, bytes, .. } => {
                read_bounded(path).is_ok_and(|current| &current == bytes)
            }
            Self::Removed { path, .. } => std::fs::symlink_metadata(path).is_err(),
        }
    }
}

/// The configuration parsed and validated by the runtime's own schema.
fn validate_config(cfg: &Value) -> Result<()> {
    let runtime: crate::config::RuntimeConfig = serde_json::from_value(cfg.clone())
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("the requested policy is not valid for this tunnel")?;
    runtime
        .validate_public()
        .context("the requested policy is not valid for this tunnel")
}

/// The edited configuration parsed, validated and serialized by the runtime's own schema.
fn validated_config(cfg: &Value) -> Result<Vec<u8>> {
    validate_config(cfg)?;
    Ok(serde_json::to_vec_pretty(cfg)?)
}

fn verify_policy(
    action: PolicyAction,
    name: &str,
    d: &Dirs<'_>,
    files: &PolicyFiles,
    new_cfg: Option<&[u8]>,
    profile: Option<&str>,
) -> Result<()> {
    d.service.reload()?;
    if files.was_up {
        d.service.restart(name)?;
    }
    let bytes = read_bounded(&files.config_path)?;
    if new_cfg.is_some_and(|want| bytes != want) {
        bail!(
            "{} does not have the bytes this request wrote",
            files.config_path.display()
        );
    }
    if new_cfg.is_none() && bytes != files.cfg_old {
        bail!("{} changed during the request", files.config_path.display());
    }
    let cfg: Value =
        serde_json::from_slice(&bytes).context("the written configuration is not valid JSON")?;
    validate_config(&cfg).context("the written configuration is not valid")?;
    let (profile_now, adaptive_now) = policy_row(&cfg)?;
    match action {
        PolicyAction::Select => {
            if profile_now.as_deref() != profile {
                bail!("policy.profile does not hold the requested id");
            }
        }
        PolicyAction::Reset => {
            if profile_now.is_some() || adaptive_now {
                bail!("policy is not cleared");
            }
        }
        _ => {}
    }
    match action {
        PolicyAction::Enable => {
            if read_bounded(&files.drop_path)?.as_slice() != MANAGED_DROP_IN {
                bail!(
                    "{} does not hold the managed content",
                    files.drop_path.display()
                );
            }
        }
        PolicyAction::Disable => {
            if std::fs::symlink_metadata(&files.drop_path).is_ok() {
                bail!("{} still exists", files.drop_path.display());
            }
        }
        _ => {}
    }
    if files.was_up {
        // Evidence from the unit itself: `systemctl show -p Environment` is what systemd applied to
        // the process it just started, and it does not need the helper to read another uid's
        // `/proc` (the installed service has no CAP_SYS_PTRACE by design).
        let switch = d.service.switch_value(name)?;
        match action {
            PolicyAction::Enable => {
                if switch.as_deref() != Some(PACK_MODE) {
                    bail!("the running unit does not have {PACK_SWITCH}={PACK_MODE}");
                }
            }
            PolicyAction::Disable => {
                if switch.is_some() {
                    bail!("the running unit still has {PACK_SWITCH} in its environment");
                }
            }
            _ => {
                if let SwitchBaseline::Known(before) = &files.baseline {
                    if &switch != before {
                        bail!(
                            "the running unit's {PACK_SWITCH} changed although this action does not touch it"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// How a restore attempt ended: verified, or verified only at the file level because the unit's
/// switch could not be read, or stopped because a file this request wrote was changed underneath it.
enum Restore {
    Verified,
    FileOnly,
    Conflict(Vec<String>),
}

#[allow(clippy::too_many_arguments)]
fn recover_policy(
    name: &str,
    req: &Request,
    d: &Dirs<'_>,
    action: PolicyAction,
    previous: &Value,
    requested: &Value,
    files: &PolicyFiles,
    written: &[Written],
    failed: String,
) -> PolicyOutcome {
    let mut restored: Vec<String> = Vec::new();
    let attempt = (|| -> std::result::Result<Restore, String> {
        let conflicts: Vec<String> = written
            .iter()
            .filter(|w| !w.holds_its_own_write())
            .map(|w| w.label().to_string())
            .collect();
        if !conflicts.is_empty() {
            // Another writer changed a file this request had already replaced. Overwriting it would
            // destroy that edit, so it is left as it is and reported.
            return Ok(Restore::Conflict(conflicts));
        }
        for w in written {
            let label = w.label();
            let path = files.path(label);
            match (w, files.previous(label)) {
                // The file did not exist before this request: putting it back means removing it.
                (Written::Replaced { .. }, None) => {
                    match std::fs::remove_file(path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(format!("restore {label}: {error:#}")),
                    }
                    restored.push(label.to_string());
                }
                (Written::Replaced { .. }, Some(previous)) => {
                    if label == files.config_name {
                        replace(path, previous, &files.cfg_meta)
                            .map_err(|e| format!("restore {label}: {e:#}"))?;
                    } else {
                        write_fixed(path, previous, 0o644)
                            .map_err(|e| format!("restore {label}: {e:#}"))?;
                    }
                    restored.push(label.to_string());
                }
                (Written::Removed { .. }, Some(previous)) => {
                    write_fixed(path, previous, 0o644)
                        .map_err(|e| format!("restore {label}: {e:#}"))?;
                    restored.push(label.to_string());
                }
                (Written::Removed { .. }, None) => {
                    return Err(format!("restore {label}: the backup has no bytes"));
                }
            }
        }
        d.service
            .reload()
            .map_err(|e| format!("systemctl daemon-reload: {e:#}"))?;
        if files.was_up {
            d.service
                .restart(name)
                .map_err(|e| format!("restart baft-tunnel@{name} on the previous files: {e:#}"))?;
        }
        for w in written {
            let label = w.label();
            match (files.previous(label), read_bounded(files.path(label))) {
                (Some(previous), Ok(now)) if now == previous => {}
                (None, Err(_)) if std::fs::symlink_metadata(files.path(label)).is_err() => {}
                _ => return Err(format!("{label} does not have its previous state")),
            }
        }
        if files.was_up {
            match &files.baseline {
                SwitchBaseline::Known(before) => {
                    let switch = d
                        .service
                        .switch_value(name)
                        .map_err(|e| format!("verify the unit's environment: {e:#}"))?;
                    if &switch != before {
                        return Err(format!(
                            "the running unit's {PACK_SWITCH} does not match its value before the request"
                        ));
                    }
                }
                // Without a reading from before the write there is no unit-level claim to make.
                SwitchBaseline::Unreadable(_) => return Ok(Restore::FileOnly),
                SwitchBaseline::Stopped => {}
            }
        }
        Ok(Restore::Verified)
    })();
    let (result, level, unconfirmed, details) = match attempt {
        Ok(Restore::Verified) => (
            "rolled_back",
            if files.was_up { "unit" } else { "file" },
            Vec::new(),
            Vec::new(),
        ),
        Ok(Restore::FileOnly) => {
            let why = match &files.baseline {
                SwitchBaseline::Unreadable(error) => error.clone(),
                _ => "the unit's switch could not be read".to_string(),
            };
            (
                "rolled_back",
                "file",
                vec!["switch".to_string()],
                vec![format!(
                    "the unit's {PACK_SWITCH} could not be read before the request ({why}), so the restart was not confirmed at the unit level"
                )],
            )
        }
        Ok(Restore::Conflict(list)) => (
            "recovery_failed",
            "none",
            list.clone(),
            vec![format!(
                "changed by another writer while this request ran: {}",
                list.join(", ")
            )],
        ),
        Err(error) => ("recovery_failed", "none", Vec::new(), vec![error]),
    };
    let recovered = result == "rolled_back";
    let mut rc = policy_receipt(
        &req.key,
        name,
        action,
        result,
        previous.clone(),
        requested.clone(),
        restored,
        files.was_up && !recovered,
        json!({
            "level": level,
            "reason": match (result, level) {
                ("rolled_back", "unit") => "the unit was restarted on the previous files and its own environment was re-read",
                ("rolled_back", _) => "the files this request replaced were restored; the unit-level environment was not confirmed",
                _ => "the restore did not verify",
            },
        }),
        Some(&files.backup),
        req.profile.as_deref(),
    );
    rc["failed_step"] = json!(failed);
    let mut file_state = serde_json::Map::new();
    for label in [files.config_name.as_str(), DROP_IN] {
        let path = files.path(label);
        let now = read_bounded(path).ok();
        let before = files.previous(label);
        let was_written = written.iter().any(|w| w.label() == label);
        let state = if unconfirmed.iter().any(|c| c == label) {
            "changed_by_other"
        } else {
            match (&now, before) {
                (None, None) => "absent",
                (Some(bytes), Some(want)) if bytes == want => "previous",
                _ if was_written => "different",
                _ => "changed_by_other",
            }
        };
        file_state.insert(label.to_string(), json!(state));
    }
    rc["file_state"] = json!(file_state);
    rc["unconfirmed"] = json!(unconfirmed);
    if !recovered {
        rc["recovery_error"] = json!(details);
        rc["operator_action"] = json!(format!(
            "inspect {} and {}; a file this request wrote changed while it ran, so it was left as it is — run \"systemctl daemon-reload\"{} once the files hold what you want",
            files.config_path.display(),
            files.drop_path.display(),
            if files.was_up {
                format!(", then \"systemctl restart baft-tunnel@{name}\"")
            } else {
                "; the unit was stopped before this request and was not started".to_string()
            }
        ));
    } else if !details.is_empty() {
        rc["recovery_note"] = json!(details);
    }
    PolicyOutcome::Done(rc)
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
            let written: Vec<&str> = manifest["written_sha256"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if written.len() != files.len() {
                bail!("backup manifest has no content hashes; refusing to revert");
            }
            // Revert only the edit's own result: every current file must still be byte-for-byte
            // what the edit wrote, so another instance's or a later change is never overwritten.
            let mut plan = Vec::new();
            for (f, want) in files.iter().zip(&written) {
                let target = d.config.join(f);
                let meta = regular(&target)?.with_context(|| format!("{f} is missing"))?;
                if sha_hex(&read_bounded(&target)?) != *want {
                    bail!("{f} changed after this edit; refusing to revert over it");
                }
                plan.push((target, read_bounded(&backup.join(f))?, meta));
            }
            for (target, bytes, meta) in &plan {
                replace(target, bytes, meta)?;
            }
            let _ = std::fs::remove_dir_all(backup);
        }
        _ => bail!("backup manifest has an unknown operation"),
    }
    Ok(files)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PeerIdentity {
    pid: i32,
    uid: u32,
    gid: u32,
}

/// The root helper accepts only the dedicated agent account, then binds that peer to the exact
/// systemd instance cgroup. The UID alone is deliberately insufficient: all agents currently
/// share this account for compatibility with ADR-0023/0026.
fn authorize_peer(
    name: &str,
    peer: PeerIdentity,
    expected_uid: u32,
    expected_gid: u32,
    cgroup: &str,
) -> Result<()> {
    if !crate::apply::valid_instance(name) {
        bail!("invalid instance name");
    }
    if peer.pid <= 0 {
        bail!("peer PID is invalid");
    }
    if peer.uid != expected_uid || peer.gid != expected_gid {
        bail!("peer UID/GID is not the dedicated BAFT-TUNNEL agent account");
    }
    let expected = format!("/system.slice/baft-tunnel-agent@{name}.service");
    let belongs_to_instance = cgroup.lines().any(|line| {
        let mut fields = line.splitn(3, ':');
        let hierarchy = fields.next().unwrap_or_default();
        let controllers = fields.next().unwrap_or_default();
        let path = fields.next().unwrap_or_default();
        let systemd_hierarchy = (hierarchy == "0" && controllers.is_empty())
            || controllers
                .split(',')
                .any(|controller| controller == "name=systemd");
        systemd_hierarchy && path == expected
    });
    if !belongs_to_instance {
        bail!("peer is not running inside baft-tunnel-agent@{name}.service");
    }
    Ok(())
}

fn expected_agent_ids() -> Result<(u32, u32)> {
    let name = CString::new("baft-tunnel-agent").expect("static account name has no NUL");
    // SAFETY: getpwnam reads the process account database; the returned static entry is copied
    // immediately and this helper handles one accepted connection at a time.
    let entry = unsafe { libc::getpwnam(name.as_ptr()) };
    if entry.is_null() {
        bail!("the dedicated baft-tunnel-agent account does not exist");
    }
    // SAFETY: entry was checked non-null above.
    let uid = unsafe { (*entry).pw_uid as u32 };
    // SAFETY: same checked passwd entry.
    let gid = unsafe { (*entry).pw_gid as u32 };
    Ok((uid, gid))
}

fn peer_cgroup(pid: i32) -> Result<String> {
    if pid <= 0 {
        bail!("peer PID is invalid");
    }
    let path = PathBuf::from(format!("/proc/{pid}/cgroup"));
    std::fs::read_to_string(&path).with_context(|| {
        format!(
            "cannot verify the connecting agent's cgroup ({})",
            path.display()
        )
    })
}

#[cfg(target_os = "linux")]
fn peer_credentials(stream: &UnixStream) -> Result<PeerIdentity> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: credentials and length point to writable storage of the sizes required by
    // SO_PEERCRED; stream owns a live AF_UNIX socket.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error()).context("read Unix peer credentials");
    }
    if length as usize != std::mem::size_of::<libc::ucred>() {
        bail!("kernel returned an unexpected SO_PEERCRED size");
    }
    Ok(PeerIdentity {
        pid: credentials.pid,
        uid: credentials.uid,
        gid: credentials.gid,
    })
}

#[cfg(not(target_os = "linux"))]
fn peer_credentials(_stream: &UnixStream) -> Result<PeerIdentity> {
    bail!("the privileged manage socket requires Linux SO_PEERCRED")
}

#[cfg(target_os = "linux")]
fn systemd_listener(expected_path: &Path) -> Result<UnixListener> {
    let listen_pid =
        std::env::var("LISTEN_PID").context("LISTEN_PID is missing (not socket-activated)")?;
    if listen_pid.parse::<u32>().ok() != Some(std::process::id()) {
        bail!("LISTEN_PID does not identify this helper process");
    }
    if std::env::var("LISTEN_FDS").ok().as_deref() != Some("1") {
        bail!("expected exactly one systemd listening file descriptor");
    }
    if std::env::var("LISTEN_FDNAMES").ok().as_deref() != Some("manage") {
        bail!("the systemd socket descriptor is not named manage");
    }
    // SAFETY: systemd passes the sole listening socket as fd 3 when LISTEN_FDS=1. Ownership is
    // transferred to UnixListener, which closes it on drop. The checks above bind it to this unit.
    let listener = unsafe { UnixListener::from_raw_fd(3) };
    let local_addr = listener.local_addr()?;
    let actual_path = local_addr.as_pathname();
    if actual_path != Some(expected_path) {
        bail!("systemd passed a socket for an unexpected instance path");
    }
    let mut accepting: libc::c_int = 0;
    let mut length = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: accepting and length are writable and fd 3 is the inherited socket.
    let rc = unsafe {
        libc::getsockopt(
            listener.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_ACCEPTCONN,
            (&mut accepting as *mut libc::c_int).cast(),
            &mut length,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error()).context("verify systemd listening socket");
    }
    if accepting != 1 {
        bail!("systemd passed a connected socket instead of a listening socket");
    }
    Ok(listener)
}

#[cfg(not(target_os = "linux"))]
fn systemd_listener(_expected_path: &Path) -> Result<UnixListener> {
    bail!("the privileged manage socket requires Linux systemd socket activation")
}

fn read_stream_bounded(stream: &mut UnixStream) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    stream
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("read manage request from Unix socket")?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        bail!("manage request is larger than {MAX_FILE_BYTES} bytes");
    }
    Ok(bytes)
}

fn refused_receipt(reason: String, request: Option<&Request>) -> Value {
    let mut receipt = json!({
        "receipt_version": 1,
        "id": request.map(|r| r.key.clone()),
        "result": "refused",
        "reason": reason,
        "files": [],
        "source": "root tunnel helper"
    });
    if let Some(request) = request {
        receipt["op"] = json!(request.op.as_str());
    }
    receipt["secrets"] =
        json!("none recorded: file names only; key material and file contents never leave root");
    receipt
}

/// The receipt of one ADR-0057 write: the helper's own result, never an "active" claim.
fn process_policy(name: &str, req: &Request, d: &Dirs<'_>) -> Value {
    match apply_policy(name, req, d) {
        Ok(PolicyOutcome::Done(receipt)) => {
            prune(&d.backups.join(name), &d.backups.join(name).join(&req.key));
            receipt
        }
        Ok(PolicyOutcome::Refused { cause, reason }) => {
            let mut receipt = refused_receipt(reason, Some(req));
            receipt["cause"] = json!(cause);
            receipt
        }
        Err(error) => refused_receipt(format!("{error:#}"), Some(req)),
    }
}

fn process_request(name: &str, req: &Request, d: &Dirs<'_>) -> Value {
    if req.op == Op::Policy {
        return process_policy(name, req, d);
    }
    let instance_dir = d.backups.join(name);
    let backup = instance_dir.join(&req.key);
    let out = private_dir(d.backups)
        .and_then(|()| private_dir(&instance_dir))
        .and_then(|()| match req.op {
            Op::Remove => op_remove(name, &req.key, d, &backup),
            Op::Edit => op_edit(name, &req.key, req, d, &backup),
            Op::Revert => op_revert(name, d, &backup),
            Op::Policy => unreachable!("policy is handled before any backup is created"),
        });
    match out {
        Ok(files) => {
            if req.op != Op::Revert {
                prune(&instance_dir, &backup);
            }
            json!({"receipt_version": 1, "id": req.key, "op": req.op.as_str(),
                "result": "applied", "files": files,
                "backup": if req.op == Op::Revert { Value::Null } else { json!(backup.to_string_lossy()) },
                "source": "root tunnel helper",
                "secrets": "none recorded: file names only; key material and file contents never leave root"})
        }
        Err(error) => refused_receipt(format!("{error:#}"), Some(req)),
    }
}

/// The same authorization function is used by the socket server and the adversarial tests.
fn handle_request(
    name: &str,
    bytes: &[u8],
    d: &Dirs<'_>,
    peer: PeerIdentity,
    auth: Result<((u32, u32), String)>,
) -> Value {
    if !crate::apply::valid_instance(name) {
        return refused_receipt("invalid instance name".into(), None);
    }
    let request: Request = match serde_json::from_slice(bytes) {
        Ok(request) => request,
        Err(error) => {
            return refused_receipt(format!("the manage request is not valid: {error}"), None);
        }
    };
    if !crate::apply::valid_key(&request.key) {
        return refused_receipt("invalid idempotency key".into(), Some(&request));
    }
    if request.op != Op::Edit
        && (request.peer_host.is_some() || request.backup_peer_hosts.is_some())
    {
        return refused_receipt("only edit takes peer addresses".into(), Some(&request));
    }
    if request.op == Op::Policy {
        let Some(action) = request.action else {
            return refused_receipt("policy needs an action".into(), Some(&request));
        };
        let profile = request.profile.as_deref();
        let valid = match action {
            PolicyAction::Select => profile.is_some_and(plausible_profile_id),
            _ => profile.is_none(),
        };
        if !valid {
            return refused_receipt(
                format!(
                    "policy {} {} a profile",
                    action.as_str(),
                    if action == PolicyAction::Select {
                        "needs"
                    } else {
                        "takes no"
                    }
                ),
                Some(&request),
            );
        }
    } else if request.action.is_some() || request.profile.is_some() {
        return refused_receipt(
            "only policy takes an action or profile".into(),
            Some(&request),
        );
    }
    let ((expected_uid, expected_gid), cgroup) = match auth {
        Ok(auth) => auth,
        Err(error) => {
            return refused_receipt(
                format!("requester identity could not be verified: {error:#}"),
                Some(&request),
            );
        }
    };
    if let Err(error) = authorize_peer(name, peer, expected_uid, expected_gid, &cgroup) {
        return refused_receipt(
            format!("unauthorized manage request: {error:#}"),
            Some(&request),
        );
    }
    process_request(name, &request, d)
}

/// Socket-activated root helper. systemd passes one listening AF_UNIX socket as fd 3; this helper
/// accepts exactly one connection, authenticates SO_PEERCRED + the caller's exact agent cgroup,
/// processes one bounded request, responds on that connection, then exits. The socket unit stays
/// active and activates a fresh oneshot helper for the next request.
pub fn serve(name: &str, d: &Dirs<'_>) -> Result<Value> {
    if !crate::apply::valid_instance(name) {
        bail!("invalid instance name");
    }
    let socket_path = d.control.join(format!("{name}.{SOCKET_SUFFIX}"));
    let listener = systemd_listener(&socket_path)?;
    let (mut stream, _) = listener
        .accept()
        .context("accept manage socket connection")?;
    stream.set_read_timeout(Some(WAIT))?;
    stream.set_write_timeout(Some(WAIT))?;
    let peer = peer_credentials(&stream)?;
    let auth = expected_agent_ids()
        .and_then(|ids| peer_cgroup(peer.pid).map(|cgroup| (ids, cgroup)))
        .and_then(|((uid, gid), cgroup)| {
            authorize_peer(name, peer, uid, gid, &cgroup).map(|()| ((uid, gid), cgroup))
        });
    // Authenticate the exact instance before reading any client-controlled bytes. An agent from
    // another instance must not be able to hold this root helper open by connecting and
    // withholding its request.
    let receipt = match auth {
        Ok(((uid, gid), cgroup_before)) => {
            let bytes = read_stream_bounded(&mut stream)?;
            // Re-read the kernel cgroup after the complete request. If the peer exited and its
            // PID was recycled while the request was in flight, a changed/missing cgroup fails
            // closed instead of authorizing the replacement process.
            let auth_after = peer_cgroup(peer.pid).and_then(|cgroup_after| {
                if cgroup_after != cgroup_before {
                    bail!("peer cgroup changed while the request was in flight");
                }
                Ok(((uid, gid), cgroup_after))
            });
            handle_request(name, &bytes, d, peer, auth_after)
        }
        Err(error) => refused_receipt(format!("unauthorized manage request: {error:#}"), None),
    };
    stream
        .write_all(receipt.to_string().as_bytes())
        .context("write manage socket receipt")?;
    stream.flush().context("flush manage socket receipt")?;
    Ok(receipt)
}

/// Agent-side interface. Tests use an in-process helper; production always uses the UDS client.
pub const UNKNOWN_OUTCOME: &str = "outcome unknown";

pub trait ManageHelper {
    fn submit(&self, name: &str, request: &Request) -> Result<Value, String>;
}

pub struct SystemManageHelper {
    pub socket_dir: PathBuf,
}

impl Default for SystemManageHelper {
    fn default() -> Self {
        Self {
            socket_dir: PathBuf::from(SOCKET_DIR),
        }
    }
}

impl ManageHelper for SystemManageHelper {
    fn submit(&self, name: &str, request: &Request) -> Result<Value, String> {
        if !crate::apply::valid_instance(name) {
            return Err("invalid instance".into());
        }
        let unit = Path::new("/etc/systemd/system/baft-tunnel-control-manage@.socket");
        let socket = self.socket_dir.join(format!("{name}.{SOCKET_SUFFIX}"));
        let socket_is_real = std::fs::symlink_metadata(&socket)
            .map(|meta| meta.file_type().is_socket())
            .unwrap_or(false);
        if !self.socket_dir.is_dir() || !unit.is_file() || !socket_is_real {
            return Err(format!(
                "the root manage socket is not installed or active; run sudo scripts/install-agent-control.sh --instance {name}"
            ));
        }
        submit_in(&self.socket_dir, name, request, WAIT)
    }
}

/// Send one bounded request to the instance-specific systemd socket and read its receipt.
pub fn submit_in(
    socket_dir: &Path,
    name: &str,
    request: &Request,
    wait: Duration,
) -> Result<Value, String> {
    let socket_path = socket_dir.join(format!("{name}.{SOCKET_SUFFIX}"));
    let mut stream = UnixStream::connect(&socket_path)
        .map_err(|error| format!("cannot connect to the root manage socket: {error}"))?;
    stream
        .set_write_timeout(Some(wait))
        .map_err(|error| format!("cannot set manage socket write timeout: {error}"))?;
    stream
        .set_read_timeout(Some(wait))
        .map_err(|error| format!("cannot set manage socket read timeout: {error}"))?;
    let body = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    stream
        .write_all(&body)
        .map_err(|error| format!("{UNKNOWN_OUTCOME}: cannot send the manage request: {error}"))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|error| format!("{UNKNOWN_OUTCOME}: cannot finish the manage request: {error}"))?;
    let mut bytes = Vec::new();
    stream
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            format!("{UNKNOWN_OUTCOME}: root manage helper did not answer: {error}")
        })?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!(
            "{UNKNOWN_OUTCOME}: helper receipt exceeded {MAX_FILE_BYTES} bytes"
        ));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("{UNKNOWN_OUTCOME}: helper returned invalid JSON: {error}"))?;
    let ours = (value["id"] == json!(request.key) && value["op"] == json!(request.op.as_str()))
        || (value["id"].is_null() && value["result"] == "refused");
    if !ours {
        return Err(UNKNOWN_OUTCOME.into());
    }
    Ok(value)
}

#[cfg(test)]
pub(crate) fn run(name: &str, d: &Dirs<'_>) -> Result<Value> {
    if !crate::apply::valid_instance(name) {
        bail!("invalid instance name");
    }
    let request_path = d.control.join(format!("{name}.{REQUEST_SUFFIX}"));
    let bytes = read_bounded(&request_path);
    let _ = std::fs::remove_file(&request_path);
    let peer = PeerIdentity {
        pid: 1234,
        uid: 1001,
        gid: 1001,
    };
    let cgroup = format!("0::/system.slice/baft-tunnel-agent@{name}.service");
    Ok(handle_request(
        name,
        &bytes?,
        d,
        peer,
        Ok(((1001, 1001), cgroup)),
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
        pub units: PathBuf,
        pub service: FakeService,
    }

    /// Unit operations without systemd: the running unit's environment is derived from the drop-in
    /// that is on disk, exactly as `systemctl restart` would load it, and failures are injectable.
    pub(crate) struct FakeService {
        pub units: PathBuf,
        pub reloads: std::sync::Mutex<Vec<()>>,
        pub restarts: std::sync::Mutex<Vec<String>>,
        pub switch: std::sync::Mutex<Option<String>>,
        pub reload_failures: std::sync::atomic::AtomicUsize,
        pub restart_failures: std::sync::atomic::AtomicUsize,
        pub switch_failures: std::sync::atomic::AtomicUsize,
    }

    impl FakeService {
        fn new(units: PathBuf) -> Self {
            Self {
                units,
                reloads: std::sync::Mutex::new(vec![]),
                restarts: std::sync::Mutex::new(vec![]),
                switch: std::sync::Mutex::new(None),
                reload_failures: std::sync::atomic::AtomicUsize::new(0),
                restart_failures: std::sync::atomic::AtomicUsize::new(0),
                switch_failures: std::sync::atomic::AtomicUsize::new(0),
            }
        }
        fn fail(&self, field: &std::sync::atomic::AtomicUsize, times: usize) {
            field.store(times, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl ServiceControl for FakeService {
        fn reload(&self) -> Result<()> {
            if self
                .reload_failures
                .fetch_update(
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                    |n| n.checked_sub(1),
                )
                .is_ok()
            {
                bail!("daemon-reload failed (test)");
            }
            self.reloads.lock().unwrap().push(());
            Ok(())
        }

        fn restart(&self, name: &str) -> Result<()> {
            if self
                .restart_failures
                .fetch_update(
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                    |n| n.checked_sub(1),
                )
                .is_ok()
            {
                bail!("restart failed (test)");
            }
            self.restarts.lock().unwrap().push(name.to_string());
            let drop = self
                .units
                .join(format!("baft-tunnel@{name}.service.d"))
                .join(DROP_IN);
            let switch = std::fs::read(&drop)
                .ok()
                .filter(|bytes| bytes == MANAGED_DROP_IN)
                .map(|_| PACK_MODE.to_string());
            *self.switch.lock().unwrap() = switch;
            Ok(())
        }

        fn switch_value(&self, _: &str) -> Result<Option<String>> {
            if self
                .switch_failures
                .fetch_update(
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                    |n| n.checked_sub(1),
                )
                .is_ok()
            {
                bail!("the environment of the running unit could not be read (test)");
            }
            Ok(self.switch.lock().unwrap().clone())
        }
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
                units: root.join("units"),
                service: FakeService::new(root.join("units")),
                root,
            };
            for p in [&e.control, &e.config, &e.run, &e.units] {
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
                units: &self.units,
                service: &self.service,
            }
        }
        /// The unit is running: the runtime marker exists and its environment already has the
        /// switch the fake would have loaded at start.
        pub(crate) fn running(&self, name: &str) {
            std::fs::create_dir_all(self.run.join(name)).unwrap();
            let drop = self
                .units
                .join(format!("baft-tunnel@{name}.service.d"))
                .join(DROP_IN);
            let switch = std::fs::read(&drop)
                .ok()
                .filter(|bytes| bytes == MANAGED_DROP_IN)
                .map(|_| PACK_MODE.to_string());
            *self.service.switch.lock().unwrap() = switch;
        }
        pub(crate) fn drop_in(&self, name: &str) -> PathBuf {
            self.units
                .join(format!("baft-tunnel@{name}.service.d"))
                .join(DROP_IN)
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
    fn revert_of_an_edit_refuses_files_changed_after_the_edit() {
        let e = Env::new("cas");
        e.forward("edge");
        e.put(
            "edge",
            &json!({"op": "edit", "key": "cas-key-001", "peer_host": "198.51.100.9"}),
        );
        assert_eq!(run("edge", &e.dirs()).unwrap()["result"], "applied");
        // Another setup rewrote the pairing state (e.g. for another instance) after the edit.
        let other = json!({"config_file": "other.json"}).to_string();
        std::fs::write(e.config.join("pairing-state.json"), &other).unwrap();
        e.put("edge", &json!({"op": "revert", "key": "cas-key-001"}));
        let out = run("edge", &e.dirs()).unwrap();
        assert_eq!(out["result"], "refused", "{out}");
        assert!(
            out["reason"]
                .as_str()
                .unwrap()
                .contains("changed after this edit")
        );
        assert_eq!(
            std::fs::read_to_string(e.config.join("pairing-state.json")).unwrap(),
            other
        );
        let cfg: Value =
            serde_json::from_slice(&std::fs::read(e.config.join("edge.json")).unwrap()).unwrap();
        assert_eq!(cfg["remote"], "198.51.100.9:8443", "nothing reverted");
    }

    #[test]
    fn remove_backups_are_never_pruned() {
        let e = Env::new("keep-rm");
        e.forward("edge");
        e.put("edge", &json!({"op": "remove", "key": "keep-rm-001"}));
        assert_eq!(run("edge", &e.dirs()).unwrap()["result"], "applied");
        e.forward("edge");
        for i in 0..4 {
            std::thread::sleep(Duration::from_millis(20));
            e.put(
                "edge",
                &json!({"op": "edit", "key": format!("keep-ed-00{i}"), "peer_host": format!("198.51.100.{}", i + 1)}),
            );
            assert_eq!(run("edge", &e.dirs()).unwrap()["result"], "applied");
        }
        assert!(e.backups.join("edge/keep-rm-001/node.noise").exists());
        assert!(
            !e.backups.join("edge/keep-ed-000").exists(),
            "old edit backup pruned"
        );
    }

    #[test]
    fn a_result_for_another_request_is_an_unknown_outcome() {
        let e = Env::new("foreign");
        let req = Request {
            op: Op::Edit,
            key: "fr-key-0001".into(),
            peer_host: Some("198.51.100.9".into()),
            backup_peer_hosts: None,
            action: None,
            profile: None,
        };
        let listener = UnixListener::bind(e.control.join(format!("edge.{SOCKET_SUFFIX}"))).unwrap();
        let t = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut body = Vec::new();
            stream.read_to_end(&mut body).unwrap();
            let _: Request = serde_json::from_slice(&body).unwrap();
            stream
                .write_all(
                    json!({"id": "other-key-01", "op": "edit", "result": "applied"})
                        .to_string()
                        .as_bytes(),
                )
                .unwrap();
        });
        let err = submit_in(&e.control, "edge", &req, Duration::from_secs(5)).unwrap_err();
        t.join().unwrap();
        assert!(err.starts_with(UNKNOWN_OUTCOME), "{err}");
    }

    #[test]
    fn submit_in_times_out_waiting_for_the_socket_helper() {
        let e = Env::new("st");
        let req = Request {
            op: Op::Remove,
            key: "sb-key-0001".into(),
            peer_host: None,
            backup_peer_hosts: None,
            action: None,
            profile: None,
        };
        let listener = UnixListener::bind(e.control.join(format!("edge.{SOCKET_SUFFIX}"))).unwrap();
        let t = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut body = Vec::new();
            stream.read_to_end(&mut body).unwrap();
            std::thread::sleep(Duration::from_millis(250));
            let _ = stream.write_all(
                json!({"id": "sb-key-0001", "op": "remove", "result": "applied"})
                    .to_string()
                    .as_bytes(),
            );
        });
        let err = submit_in(&e.control, "edge", &req, Duration::from_millis(50)).unwrap_err();
        t.join().unwrap();
        assert!(err.contains("did not answer"), "{err}");
    }

    #[test]
    fn agent_a_cannot_remove_edit_or_revert_instance_b_even_with_the_shared_uid() {
        let peer = PeerIdentity {
            pid: 4321,
            uid: 1001,
            gid: 1001,
        };
        let cgroup_a = "0::/system.slice/baft-tunnel-agent@a.service".to_string();
        let key = "cross-key-001";

        for op in [Op::Remove, Op::Edit, Op::Revert] {
            let e = Env::new(&format!("cross-instance-{}", op.as_str()));
            e.forward("b");
            let backup = e.backups.join(format!("b/{key}"));
            if op == Op::Revert {
                // Create a genuine B-owned edit backup so the attempted revert has a target.
                e.put(
                    "b",
                    &json!({"op": "edit", "key": key, "peer_host": "198.51.100.9"}),
                );
                assert_eq!(run("b", &e.dirs()).unwrap()["result"], "applied");
                assert!(backup.is_dir());
            }
            let config = e.config.join("b.json");
            let before = std::fs::read(&config).unwrap();
            let request = Request {
                op,
                key: key.into(),
                peer_host: (op == Op::Edit).then(|| "198.51.100.10".into()),
                backup_peer_hosts: None,
                action: None,
                profile: None,
            };
            let bytes = serde_json::to_vec(&request).unwrap();
            let receipt = handle_request(
                "b",
                &bytes,
                &e.dirs(),
                peer,
                Ok(((1001, 1001), cgroup_a.clone())),
            );
            assert_eq!(receipt["result"], "refused", "{op:?}: {receipt}");
            assert!(
                receipt["reason"]
                    .as_str()
                    .unwrap()
                    .contains("not running inside")
            );
            assert_eq!(
                std::fs::read(&config).unwrap(),
                before,
                "{op:?} must not alter B config"
            );
            assert_eq!(
                backup.is_dir(),
                op == Op::Revert,
                "{op:?} must not create or consume B backup"
            );
        }
    }

    #[test]
    fn peer_authorization_requires_the_exact_systemd_instance_cgroup() {
        let peer = PeerIdentity {
            pid: 1234,
            uid: 1001,
            gid: 1001,
        };
        assert!(
            authorize_peer(
                "edge",
                peer,
                1001,
                1001,
                "0::/system.slice/baft-tunnel-agent@edge.service"
            )
            .is_ok()
        );
        assert!(
            authorize_peer(
                "edge",
                peer,
                1001,
                1001,
                "1:name=systemd:/system.slice/baft-tunnel-agent@edge.service"
            )
            .is_ok()
        );
        assert!(
            authorize_peer(
                "edge",
                peer,
                1001,
                1001,
                "0::/system.slice/baft-tunnel-agent@edge.service-evil"
            )
            .is_err()
        );
        assert!(
            authorize_peer(
                "edge",
                peer,
                1002,
                1001,
                "0::/system.slice/baft-tunnel-agent@edge.service"
            )
            .is_err()
        );
        assert!(
            authorize_peer(
                "edge",
                peer,
                1001,
                1002,
                "0::/system.slice/baft-tunnel-agent@edge.service"
            )
            .is_err()
        );
    }

    fn policy(e: &Env, name: &str, action: &str, profile: Option<&str>, key: &str) -> Value {
        let mut req = json!({"op": "policy", "action": action, "key": key});
        if let Some(id) = profile {
            req["profile"] = json!(id);
        }
        e.put(name, &req);
        run(name, &e.dirs()).unwrap()
    }

    #[test]
    fn policy_enable_writes_the_managed_drop_in_and_disable_removes_only_that() {
        let e = Env::new("pol-switch");
        e.forward("edge");
        let out = policy(&e, "edge", "enable", None, "po-key-0001");
        assert_eq!(out["result"], "configured", "{out}");
        assert_eq!(out["action"], "enable");
        assert_eq!(out["previous"]["switch"], Value::Null);
        assert_eq!(out["requested"]["switch"], PACK_MODE);
        assert_eq!(std::fs::read(e.drop_in("edge")).unwrap(), MANAGED_DROP_IN);
        // The unit was stopped, so nothing was started and the verification is file-level.
        assert_eq!(out["verification"]["level"], "file");
        assert_eq!(out["restart_required"], true);
        assert!(e.service.restarts.lock().unwrap().is_empty());
        // Converges without writing again and without touching systemd.
        let reloads = e.service.reloads.lock().unwrap().len();
        let again = policy(&e, "edge", "enable", None, "po-key-0002");
        assert_eq!(again["result"], "unchanged", "{again}");
        assert_eq!(e.service.reloads.lock().unwrap().len(), reloads);
        assert!(e.service.restarts.lock().unwrap().is_empty());
        assert_eq!(again["files"], json!([]));
        let cfg_before = std::fs::read(e.config.join("edge.json")).unwrap();
        let off = policy(&e, "edge", "disable", None, "po-key-0003");
        assert_eq!(off["result"], "configured", "{off}");
        assert!(!e.drop_in("edge").exists());
        assert_eq!(
            std::fs::read(e.config.join("edge.json")).unwrap(),
            cfg_before
        );
        let off_again = policy(&e, "edge", "disable", None, "po-key-0004");
        assert_eq!(off_again["result"], "unchanged", "{off_again}");
    }

    #[test]
    fn policy_refuses_a_switch_it_does_not_own_and_never_edits_it() {
        let e = Env::new("pol-foreign");
        e.forward("edge");
        std::fs::create_dir_all(e.drop_in("edge").parent().unwrap()).unwrap();
        let foreign = b"[Service]\nEnvironment=BAFT_TUNNEL_POLICY_PACK=sched BAFT_X=1\n";
        std::fs::write(e.drop_in("edge"), foreign).unwrap();
        let out = policy(&e, "edge", "enable", None, "po-key-0011");
        assert_eq!(out["result"], "refused", "{out}");
        assert_eq!(out["cause"], "switch_managed_elsewhere");
        assert_eq!(std::fs::read(e.drop_in("edge")).unwrap(), foreign);
        let out = policy(&e, "edge", "disable", None, "po-key-0012");
        assert_eq!(out["result"], "refused", "{out}");
        assert_eq!(out["cause"], "switch_not_managed");
        assert_eq!(std::fs::read(e.drop_in("edge")).unwrap(), foreign);
        // A unit-file switch is reported, not edited.
        std::fs::remove_file(e.drop_in("edge")).unwrap();
        std::fs::write(
            e.units.join("baft-tunnel@edge.service"),
            "[Service]\nEnvironment=BAFT_TUNNEL_POLICY_PACK=sched\n",
        )
        .unwrap();
        let out = policy(&e, "edge", "enable", None, "po-key-0013");
        assert_eq!(out["result"], "refused", "{out}");
        assert_eq!(out["cause"], "switch_managed_elsewhere");
        assert!(!e.drop_in("edge").exists());
    }

    #[test]
    fn policy_select_writes_the_runtime_validated_profile_and_preserves_adaptive() {
        let e = Env::new("pol-select");
        e.forward("edge");
        let mut cfg: Value =
            serde_json::from_slice(&std::fs::read(e.config.join("edge.json")).unwrap()).unwrap();
        cfg["policy"] = json!({"adaptive": true});
        std::fs::write(e.config.join("edge.json"), cfg.to_string()).unwrap();
        let out = policy(&e, "edge", "select", Some("gaming/1"), "po-key-0021");
        assert_eq!(out["result"], "configured", "{out}");
        assert_eq!(out["previous"]["profile"], Value::Null);
        assert_eq!(out["previous"]["adaptive"], true);
        assert_eq!(out["requested"]["profile"], "gaming/1");
        let cfg: Value =
            serde_json::from_slice(&std::fs::read(e.config.join("edge.json")).unwrap()).unwrap();
        assert_eq!(cfg["policy"]["profile"], "gaming/1");
        assert_eq!(cfg["policy"]["adaptive"], true, "adaptive is preserved");
        let same = policy(&e, "edge", "select", Some("gaming/1"), "po-key-0022");
        assert_eq!(same["result"], "unchanged", "{same}");
        // An id the runtime does not know is refused by the runtime's own list.
        let bad = policy(&e, "edge", "select", Some("baft/1"), "po-key-0023");
        assert_eq!(bad["result"], "refused", "{bad}");
        assert_eq!(bad["cause"], "policy_not_valid");
        let reason = bad["reason"].as_str().unwrap();
        assert!(reason.contains("balanced/1"), "{reason}");
        let cfg: Value =
            serde_json::from_slice(&std::fs::read(e.config.join("edge.json")).unwrap()).unwrap();
        assert_eq!(cfg["policy"]["profile"], "gaming/1");
        let reset = policy(&e, "edge", "reset", None, "po-key-0024");
        assert_eq!(reset["result"], "configured", "{reset}");
        let cfg: Value =
            serde_json::from_slice(&std::fs::read(e.config.join("edge.json")).unwrap()).unwrap();
        assert!(
            cfg.get("policy").is_none(),
            "an emptied policy object is dropped"
        );
        let reset = policy(&e, "edge", "reset", None, "po-key-0025");
        assert_eq!(reset["result"], "unchanged", "{reset}");
    }

    #[test]
    fn policy_restarts_a_running_unit_and_verifies_its_environment() {
        let e = Env::new("pol-up");
        e.forward("edge");
        e.running("edge");
        let out = policy(&e, "edge", "enable", None, "po-key-0031");
        assert_eq!(out["result"], "configured", "{out}");
        assert_eq!(out["verification"]["level"], "unit");
        assert_eq!(*e.service.restarts.lock().unwrap(), ["edge"]);
        assert_eq!(e.service.switch.lock().unwrap().as_deref(), Some(PACK_MODE));
        let out = policy(&e, "edge", "disable", None, "po-key-0032");
        assert_eq!(out["result"], "configured", "{out}");
        assert_eq!(e.service.switch.lock().unwrap().as_deref(), None);
    }

    #[test]
    fn policy_rolls_back_both_files_when_the_new_state_does_not_verify() {
        let e = Env::new("pol-rollback");
        e.forward("edge");
        e.running("edge");
        let cfg_before = std::fs::read(e.config.join("edge.json")).unwrap();
        // The restart done by verification fails, so the helper must put both files back.
        e.service.fail(&e.service.restart_failures, 1);
        let out = policy(&e, "edge", "enable", None, "po-key-0041");
        assert_eq!(out["result"], "rolled_back", "{out}");
        assert!(out["failed_step"].as_str().unwrap().contains("restart"));
        assert_eq!(
            out["file_state"],
            json!({"edge.json": "previous", DROP_IN: "absent"})
        );
        assert_eq!(
            std::fs::read(e.config.join("edge.json")).unwrap(),
            cfg_before
        );
        assert!(!e.drop_in("edge").exists());
        assert_eq!(out["verification"]["level"], "unit");
    }

    #[test]
    fn policy_reports_recovery_failure_honestly_instead_of_success() {
        let e = Env::new("pol-recovery");
        e.forward("edge");
        e.running("edge");
        // Every restart fails: the rollback cannot bring the unit back on the previous files.
        e.service.fail(&e.service.restart_failures, 2);
        let out = policy(&e, "edge", "enable", None, "po-key-0051");
        assert_eq!(out["result"], "recovery_failed", "{out}");
        let errors = out["recovery_error"].as_array().unwrap();
        assert!(
            errors
                .iter()
                .any(|e| e.as_str().unwrap().contains("restart")),
            "{out}"
        );
        assert!(
            out["operator_action"]
                .as_str()
                .unwrap()
                .contains("systemctl")
        );
        assert_eq!(out["verification"]["level"], "none");
        assert_eq!(out["file_state"]["edge.json"], "previous");
        assert_eq!(
            std::fs::read(e.config.join("edge.json")).unwrap(),
            std::fs::read(e.config.join("edge.json")).unwrap()
        );
    }

    #[test]
    fn policy_requests_take_no_peer_addresses_and_a_profile_only_for_select() {
        let e = Env::new("pol-bad");
        e.forward("edge");
        for req in [
            json!({"op": "policy", "key": "po-bad-0001"}),
            json!({"op": "policy", "key": "po-bad-0002", "action": "select"}),
            json!({"op": "policy", "key": "po-bad-0003", "action": "select", "profile": "gaming/1",
                "peer_host": "198.51.100.9"}),
            json!({"op": "policy", "key": "po-bad-0004", "action": "enable", "profile": "gaming/1"}),
            json!({"op": "policy", "key": "po-bad-0005", "action": "disable", "profile": "x"}),
            json!({"op": "policy", "key": "po-bad-0006", "action": "enable", "listen": "0.0.0.0:1"}),
            json!({"op": "edit", "key": "po-bad-0007", "action": "enable",
                "peer_host": "198.51.100.9"}),
        ] {
            e.put("edge", &req);
            let out = run("edge", &e.dirs()).unwrap();
            assert_eq!(out["result"], "refused", "{req}: {out}");
        }
        assert!(!e.drop_in("edge").exists());
    }
    #[test]
    fn only_our_own_variable_is_read_from_the_unit_environment() {
        assert_eq!(
            pack_switch_in_environment("A=1 BAFT_TUNNEL_POLICY_PACK=sched X=2").as_deref(),
            Some("sched")
        );
        assert_eq!(
            pack_switch_in_environment("A=1 BAFT_TUNNEL_POLICY_PACK=\"sched\" X=2").as_deref(),
            Some("sched")
        );
        assert_eq!(pack_switch_in_environment("A=1 B=2"), None);
        assert_eq!(pack_switch_in_environment("BAFT_TUNNEL_POLICY_PACK="), None);
    }

    #[test]
    fn a_switch_in_the_template_drop_in_counts_as_managed_elsewhere() {
        let e = Env::new("pol-template");
        e.forward("edge");
        let dir = e.units.join("baft-tunnel@.service.d");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("10-operator.conf"),
            "[Service]\nEnvironment=BAFT_TUNNEL_POLICY_PACK=sched\n",
        )
        .unwrap();
        let out = policy(&e, "edge", "enable", None, "po-key-0061");
        assert_eq!(out["result"], "refused", "{out}");
        assert_eq!(out["cause"], "switch_managed_elsewhere");
        assert!(out["reason"].as_str().unwrap().contains("10-operator.conf"));
        assert!(!e.drop_in("edge").exists());
        assert!(!e.backups.join("edge").exists(), "nothing was written");
    }

    #[test]
    fn an_invalid_configuration_on_disk_is_refused_before_anything_is_written() {
        let e = Env::new("pol-invalid");
        e.forward("edge");
        let mut cfg: Value =
            serde_json::from_slice(&std::fs::read(e.config.join("edge.json")).unwrap()).unwrap();
        cfg["engine"] = json!("not-an-engine");
        let broken = cfg.to_string();
        std::fs::write(e.config.join("edge.json"), &broken).unwrap();
        let out = policy(&e, "edge", "enable", None, "po-key-0062");
        assert_eq!(out["result"], "refused", "{out}");
        assert_eq!(out["cause"], "policy_not_valid");
        assert!(
            out["reason"]
                .as_str()
                .unwrap()
                .contains("nothing was changed")
        );
        assert_eq!(
            std::fs::read_to_string(e.config.join("edge.json")).unwrap(),
            broken
        );
        assert!(!e.drop_in("edge").exists());
        assert!(!e.backups.join("edge").exists(), "nothing was backed up");
        assert!(e.service.restarts.lock().unwrap().is_empty());
    }

    #[test]
    fn a_switch_that_cannot_be_read_before_the_write_is_not_claimed_after_recovery() {
        let e = Env::new("pol-unreadable");
        e.forward("edge");
        e.running("edge");
        // The baseline read and the post-recovery read both fail; the verification restart fails.
        e.service.fail(&e.service.switch_failures, 2);
        e.service.fail(&e.service.restart_failures, 1);
        let out = policy(&e, "edge", "enable", None, "po-key-0063");
        assert_eq!(out["result"], "rolled_back", "{out}");
        assert_eq!(out["verification"]["level"], "file", "{out}");
        assert_eq!(out["unconfirmed"], json!(["switch"]), "{out}");
        assert!(!e.drop_in("edge").exists(), "the new file was removed");
        assert_eq!(
            std::fs::read(e.config.join("edge.json")).unwrap(),
            std::fs::read(e.config.join("edge.json")).unwrap()
        );
    }

    #[test]
    fn recovery_refuses_to_overwrite_a_file_changed_while_the_request_ran() {
        let e = Env::new("pol-conflict");
        e.forward("edge");
        e.running("edge");
        let edited = b"[Service]\nEnvironment=BAFT_TUNNEL_POLICY_PACK=sched\n# operator edit\n";
        std::fs::create_dir_all(e.drop_in("edge").parent().unwrap()).unwrap();
        std::fs::write(e.drop_in("edge"), edited).unwrap();
        let cfg_path = e.config.join("edge.json");
        let cfg = std::fs::read(&cfg_path).unwrap();
        let files = PolicyFiles {
            config_name: "edge.json".into(),
            config_path: cfg_path.clone(),
            cfg_old: cfg.clone(),
            cfg_meta: std::fs::metadata(&cfg_path).unwrap(),
            drop_path: e.drop_in("edge"),
            drop_old: Some(MANAGED_DROP_IN.to_vec()),
            was_up: true,
            baseline: SwitchBaseline::Known(Some(PACK_MODE.to_string())),
            backup: e.backups.join("edge").join("po-key-0064"),
        };
        let written = [Written::replaced(
            DROP_IN.to_string(),
            e.drop_in("edge"),
            MANAGED_DROP_IN,
        )];
        let req = Request {
            op: Op::Policy,
            key: "po-key-0064".into(),
            peer_host: None,
            backup_peer_hosts: None,
            action: Some(PolicyAction::Enable),
            profile: None,
        };
        let out = recover_policy(
            "edge",
            &req,
            &e.dirs(),
            PolicyAction::Enable,
            &json!({"switch": null, "profile": null, "adaptive": false}),
            &json!({"switch": PACK_MODE, "profile": null, "adaptive": false}),
            &files,
            &written,
            "write: test".into(),
        );
        let PolicyOutcome::Done(out) = out else {
            panic!("expected a receipt");
        };
        assert_eq!(out["result"], "recovery_failed", "{out}");
        assert_eq!(out["file_state"][DROP_IN], "changed_by_other", "{out}");
        assert_eq!(std::fs::read(e.drop_in("edge")).unwrap(), edited);
        assert_eq!(std::fs::read(&cfg_path).unwrap(), cfg);
        assert!(
            e.service.restarts.lock().unwrap().is_empty(),
            "a conflicted restore does not restart the unit"
        );
    }
}
