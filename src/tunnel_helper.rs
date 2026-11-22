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
            (m.is_dir() && e.path() != keep && man["op"] == "edit").then(|| (t, e.path()))
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
    let actual_path = listener.local_addr()?.as_pathname().map(PathBuf::as_path);
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

fn process_request(name: &str, req: &Request, d: &Dirs<'_>) -> Value {
    let instance_dir = d.backups.join(name);
    let backup = instance_dir.join(&req.key);
    let out = private_dir(d.backups)
        .and_then(|()| private_dir(&instance_dir))
        .and_then(|()| match req.op {
            Op::Remove => op_remove(name, &req.key, d, &backup),
            Op::Edit => op_edit(name, &req.key, req, d, &backup),
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
}
