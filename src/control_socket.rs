//! Per-instance Agent-to-tunnel control socket (F0 / ADR-0028).
//!
//! The socket exposes one fixed operation only. Linux peer credentials are authoritative;
//! filesystem mode and group membership are defense in depth.

use std::path::{Path, PathBuf};
#[cfg(any(target_os = "linux", test))]
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};

#[cfg(any(target_os = "linux", test))]
use crate::integrity_probe::ProbeResult;
use crate::integrity_probe::{ProbeControl, ProbeLimiter};

const REQUEST: &[u8] = b"BAFT-PROBE-V2";
const MAX_RESPONSE_BYTES: usize = 4096;
const CONTROL_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProbeControlResponse {
    pub result: String,
    pub reason: Option<String>,
    pub bytes: usize,
    pub sha256_match: bool,
    pub duration_ms: Option<u128>,
    pub timestamp: u64,
}

impl ProbeControlResponse {
    #[cfg(any(target_os = "linux", test))]
    fn from_result(result: Result<ProbeResult>) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |value| value.as_secs());
        match result {
            Ok(result) => Self {
                result: "pass".into(),
                reason: None,
                bytes: result.bytes,
                sha256_match: true,
                duration_ms: Some(result.duration_ms),
                timestamp,
            },
            Err(error) => {
                let text = format!("{error:#}");
                let (result, reason) = if text.contains("mismatch") {
                    ("fail", "integrity_mismatch")
                } else if text.contains("malformed")
                    || text.contains("invalid integrity probe")
                    || text.contains("trailing")
                    || text.contains("unsupported integrity probe response")
                {
                    ("fail", "invalid_probe_response")
                } else if text.contains("rate limit") {
                    ("unknown", "rate_limited")
                } else if text.contains("already active") || text.contains("busy") {
                    ("unknown", "probe_busy")
                } else if text.contains("unavailable") || text.contains("session ended") {
                    ("unknown", "peer_unavailable")
                } else if text.contains("protocol v2") {
                    ("unknown", "unsupported_protocol")
                } else if text.contains("timed out") || text.contains("deadline") {
                    ("unknown", "timeout")
                } else {
                    ("unknown", "probe_failed")
                };
                Self {
                    result: result.into(),
                    reason: Some(reason.into()),
                    bytes: 0,
                    sha256_match: false,
                    duration_ms: None,
                    timestamp,
                }
            }
        }
    }
}

/// Agent-side client. The API supplies no destination, size, command or config path.
#[cfg(unix)]
pub async fn request(path: &Path) -> Result<ProbeControlResponse> {
    use tokio::net::UnixStream;

    let mut stream = timeout(CONTROL_IO_TIMEOUT, UnixStream::connect(path))
        .await
        .context("control socket connect timeout")??;
    stream.write_all(REQUEST).await?;
    stream.flush().await?;
    stream.shutdown().await?;

    let mut length = [0u8; 2];
    timeout(CONTROL_IO_TIMEOUT, stream.read_exact(&mut length))
        .await
        .context("control socket response header timeout")??;
    let length = u16::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_RESPONSE_BYTES {
        bail!("control socket returned an invalid response length");
    }
    let mut body = vec![0u8; length];
    timeout(CONTROL_IO_TIMEOUT, stream.read_exact(&mut body))
        .await
        .context("control socket response body timeout")??;
    let mut extra = [0u8; 1];
    if timeout(CONTROL_IO_TIMEOUT, stream.read(&mut extra))
        .await
        .context("control socket response close timeout")??
        != 0
    {
        bail!("control socket response contains trailing bytes");
    }
    let response: ProbeControlResponse =
        serde_json::from_slice(&body).context("decode control socket response")?;
    validate_response(&response)?;
    Ok(response)
}

fn validate_response(response: &ProbeControlResponse) -> Result<()> {
    match response.result.as_str() {
        "pass" => {
            if response.reason.is_some()
                || response.bytes != crate::integrity_probe::PROBE_BYTES
                || !response.sha256_match
                || response.timestamp == 0
                || response
                    .duration_ms
                    .is_none_or(|duration| duration > 10_000)
            {
                bail!("control socket returned an invalid pass receipt");
            }
        }
        "fail" | "unknown" => {
            let known_reason = match response.result.as_str() {
                "fail" => matches!(
                    response.reason.as_deref(),
                    Some("integrity_mismatch" | "invalid_probe_response")
                ),
                _ => matches!(
                    response.reason.as_deref(),
                    Some(
                        "rate_limited"
                            | "probe_busy"
                            | "peer_unavailable"
                            | "unsupported_protocol"
                            | "timeout"
                            | "probe_failed"
                    )
                ),
            };
            if !known_reason
                || response.bytes != 0
                || response.sha256_match
                || response.duration_ms.is_some()
                || response.timestamp == 0
            {
                bail!("control socket returned an invalid non-pass receipt");
            }
        }
        _ => bail!("control socket returned an unsupported result"),
    }
    Ok(())
}

#[cfg(not(unix))]
pub async fn request(_path: &Path) -> Result<ProbeControlResponse> {
    bail!("Unix-domain control socket is unavailable on this platform")
}

/// Bound per-instance listener. Binding and filesystem validation complete before the
/// tunnel's main loop begins, so a configured B2 control path cannot fail silently.
#[cfg(target_os = "linux")]
pub struct ControlServer {
    path: PathBuf,
    listener: tokio::net::UnixListener,
    expected_uid: u32,
}

#[cfg(target_os = "linux")]
impl ControlServer {
    pub fn bind(path: PathBuf) -> Result<Self> {
        let expected_uid = account_uid("baft-tunnel-agent")?;
        Self::bind_with_expected_uid(path, expected_uid, Path::new("/run/baft-tunnel"))
    }

    #[cfg(test)]
    pub(crate) fn bind_for_test(
        path: PathBuf,
        expected_uid: u32,
        runtime_root: &Path,
    ) -> Result<Self> {
        Self::bind_with_expected_uid(path, expected_uid, runtime_root)
    }

    fn bind_with_expected_uid(
        path: PathBuf,
        expected_uid: u32,
        runtime_root: &Path,
    ) -> Result<Self> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        use tokio::net::UnixListener;

        prepare_socket_path_under(&path, runtime_root, unsafe { libc::geteuid() }, unsafe {
            libc::getegid()
        })?;
        let listener = UnixListener::bind(&path)
            .with_context(|| format!("bind per-instance control socket {}", path.display()))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660))?;
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.gid() != unsafe { libc::getegid() }
            || metadata.mode() & 0o777 != 0o660
            || metadata.mode() & libc::S_IFMT != libc::S_IFSOCK
        {
            let _ = std::fs::remove_file(&path);
            bail!("control socket ownership or mode is unsafe");
        }
        Ok(Self {
            path,
            listener,
            expected_uid,
        })
    }

    pub async fn run(
        self,
        control: std::sync::Arc<ProbeControl>,
        limiter: std::sync::Arc<ProbeLimiter>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        use tokio::sync::Semaphore;

        let connections = std::sync::Arc::new(Semaphore::new(4));
        let mut tasks = tokio::task::JoinSet::new();
        let result: Result<()> = async {
            loop {
                while tasks.try_join_next().is_some() {}
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() { break; }
                    }
                    accepted = self.listener.accept() => {
                        let (stream, _) = accepted?;
                        let Ok(permit) = connections.clone().try_acquire_owned() else {
                            drop(stream);
                            continue;
                        };
                        let control = control.clone();
                        let limiter = limiter.clone();
                        let expected_uid = self.expected_uid;
                        tasks.spawn(async move {
                            let _permit = permit;
                            if let Err(error) = serve_connection(stream, expected_uid, control, limiter).await {
                                // Never log payloads, digests, API tokens or raw peer-controlled frames.
                                eprintln!("integrity control request refused: {}", safe_reason(&error));
                            }
                        });
                    }
                }
            }
            Ok(())
        }.await;

        // Stop accepting first, then give bounded in-flight requests time to finish.
        if timeout(CONTROL_IO_TIMEOUT, async {
            while tasks.join_next().await.is_some() {}
        })
        .await
        .is_err()
        {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        }
        remove_owned_socket(&self.path);
        result
    }
}

#[cfg(not(target_os = "linux"))]
pub struct ControlServer;

#[cfg(not(target_os = "linux"))]
impl ControlServer {
    pub fn bind(_path: PathBuf) -> Result<Self> {
        bail!("SO_PEERCRED control socket is supported only on Linux")
    }

    #[allow(dead_code)]
    pub async fn run(
        self,
        _control: std::sync::Arc<ProbeControl>,
        _limiter: std::sync::Arc<ProbeLimiter>,
        _shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        bail!("SO_PEERCRED control socket is supported only on Linux")
    }
}

#[cfg(target_os = "linux")]
async fn serve_connection(
    mut stream: tokio::net::UnixStream,
    expected_uid: u32,
    control: std::sync::Arc<ProbeControl>,
    limiter: std::sync::Arc<ProbeLimiter>,
) -> Result<()> {
    let uid = peer_uid(&stream)?;
    if !peer_uid_allowed(uid, expected_uid) {
        bail!("unauthorized peer uid");
    }
    let mut request = vec![0u8; REQUEST.len()];
    timeout(CONTROL_IO_TIMEOUT, stream.read_exact(&mut request))
        .await
        .context("control request read timeout")??;
    if request != REQUEST {
        bail!("unsupported control operation");
    }
    let mut extra = [0u8; 1];
    if timeout(CONTROL_IO_TIMEOUT, stream.read(&mut extra))
        .await
        .context("control request close timeout")??
        != 0
    {
        bail!("control request contains trailing bytes");
    }
    let result = match limiter.acquire().await {
        Ok(_permit) => control.request().await,
        Err(error) => Err(error),
    };
    let response = ProbeControlResponse::from_result(result);
    let body = serde_json::to_vec(&response)?;
    if body.is_empty() || body.len() > MAX_RESPONSE_BYTES {
        bail!("control response exceeds fixed size limit");
    }
    let length = u16::try_from(body.len())?;
    stream.write_all(&length.to_be_bytes()).await?;
    stream.write_all(&body).await?;
    stream.flush().await?;
    stream.shutdown().await?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &tokio::net::UnixStream) -> Result<u32> {
    use std::os::fd::AsRawFd;

    // SAFETY: getsockopt writes one initialized ucred value to the supplied buffer.
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: fd is a live Unix stream and both pointers reference writable storage.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut credentials as *mut _ as *mut libc::c_void,
            &mut length,
        )
    };
    if result != 0 || length as usize != std::mem::size_of::<libc::ucred>() {
        bail!("SO_PEERCRED is unavailable; refusing control request");
    }
    if credentials.pid <= 0 {
        bail!("invalid control peer process identity");
    }
    Ok(credentials.uid)
}

#[cfg(any(target_os = "linux", test))]
fn peer_uid_allowed(actual_uid: u32, expected_uid: u32) -> bool {
    actual_uid == expected_uid
}

#[cfg(target_os = "linux")]
fn account_uid(name: &str) -> Result<u32> {
    let passwd = std::fs::read_to_string("/etc/passwd").context("read local account database")?;
    for line in passwd.lines() {
        let mut fields = line.split(':');
        if fields.next() == Some(name) {
            let uid = fields
                .nth(1)
                .context("account record has no uid")?
                .parse::<u32>()
                .context("account uid is invalid")?;
            if uid == 0 || uid == unsafe { libc::geteuid() } {
                bail!("baft-tunnel-agent UID must be non-root and distinct from the tunnel UID");
            }
            return Ok(uid);
        }
    }
    bail!("required baft-tunnel-agent account is missing")
}

#[cfg(target_os = "linux")]
fn prepare_socket_path_under(
    path: &Path,
    root: &Path,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<()> {
    use std::{
        ffi::CString,
        os::{fd::RawFd, unix::ffi::OsStrExt, unix::fs::MetadataExt},
    };

    let parent = path
        .parent()
        .context("control socket has no parent directory")?;
    let file_name = path
        .file_name()
        .context("control socket has no file name")?;
    let instance = parent
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !path.is_absolute()
        || file_name != "control.sock"
        || parent.parent() != Some(root)
        || !crate::apply::valid_instance(instance)
    {
        bail!("control socket must be inside /run/baft-tunnel/%i");
    }

    // Pin the trusted root, then resolve the instance directory relative to that open fd.
    // This prevents an ancestor or instance-directory symlink from redirecting cleanup.
    let root_meta = std::fs::symlink_metadata(root)?;
    if !root_meta.is_dir() || root_meta.file_type().is_symlink() || root_meta.mode() & 0o022 != 0 {
        bail!("control socket root directory is unsafe");
    }
    let root_name = CString::new(root.as_os_str().as_bytes())?;
    let instance_name = CString::new(instance)?;
    let socket_name = CString::new("control.sock")?;
    // SAFETY: root_name is NUL terminated and O_NOFOLLOW rejects a symlink at the root itself.
    let root_fd: RawFd = unsafe {
        libc::open(
            root_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if root_fd < 0 {
        bail!("open trusted control socket root failed");
    }
    // SAFETY: root_fd is a live directory fd and instance_name is a single validated component.
    let instance_fd = unsafe {
        libc::openat(
            root_fd,
            instance_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if instance_fd < 0 {
        unsafe { libc::close(root_fd) };
        bail!("open per-instance runtime directory without following symlinks failed");
    }

    let result = (|| -> Result<()> {
        let mut directory: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: instance_fd is live and directory is writable.
        if unsafe { libc::fstat(instance_fd, &mut directory) } != 0 {
            return Err(std::io::Error::last_os_error()).context("inspect runtime directory");
        }
        if directory.st_mode & libc::S_IFMT != libc::S_IFDIR
            || directory.st_uid != expected_uid
            || directory.st_gid != expected_gid
            || directory.st_mode & 0o777 != 0o750
        {
            bail!("runtime directory ownership, type or permissions are unsafe");
        }

        let mut existing: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: instance_fd and socket_name are valid; existing is writable.
        let status = unsafe {
            libc::fstatat(
                instance_fd,
                socket_name.as_ptr(),
                &mut existing,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if status == 0 {
            if existing.st_mode & libc::S_IFMT != libc::S_IFSOCK
                || existing.st_uid != unsafe { libc::geteuid() }
            {
                bail!("refusing to replace a non-socket or foreign-owned control path");
            }
            // SAFETY: the entry was verified as an owned socket in the pinned directory.
            if unsafe { libc::unlinkat(instance_fd, socket_name.as_ptr(), 0) } != 0 {
                return Err(std::io::Error::last_os_error()).context("remove stale control socket");
            }
        } else {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(error).context("inspect existing control socket");
            }
        }
        Ok(())
    })();

    // SAFETY: each fd was returned by open/openat and is closed exactly once.
    unsafe {
        libc::close(instance_fd);
        libc::close(root_fd);
    }
    result
}

#[cfg(target_os = "linux")]
fn remove_owned_socket(path: &Path) {
    use std::os::unix::fs::MetadataExt;
    if let Ok(metadata) = std::fs::symlink_metadata(path)
        && metadata.mode() & libc::S_IFMT == libc::S_IFSOCK
        && metadata.uid() == unsafe { libc::geteuid() }
    {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(target_os = "linux")]
fn safe_reason(error: &anyhow::Error) -> &'static str {
    let text = format!("{error:#}");
    if text.contains("uid") || text.contains("SO_PEERCRED") {
        "peer_identity"
    } else if text.contains("timeout") {
        "timeout"
    } else if text.contains("unsupported") || text.contains("invalid") {
        "invalid_request"
    } else {
        "request_failed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn probe_failures_are_distinct_from_unknown_transport_outcomes() {
        let mismatch = ProbeControlResponse::from_result(Err(anyhow::anyhow!(
            "integrity probe SHA-256 mismatch"
        )));
        assert_eq!(mismatch.result, "fail");
        assert_eq!(mismatch.reason.as_deref(), Some("integrity_mismatch"));
        validate_response(&mismatch).unwrap();

        let unavailable = ProbeControlResponse::from_result(Err(anyhow::anyhow!(
            "authenticated tunnel session is unavailable"
        )));
        assert_eq!(unavailable.result, "unknown");
        assert_eq!(unavailable.reason.as_deref(), Some("peer_unavailable"));
        validate_response(&unavailable).unwrap();

        let invalid = ProbeControlResponse {
            result: "fail".into(),
            reason: Some("made_up".into()),
            bytes: 0,
            sha256_match: false,
            duration_ms: None,
            timestamp: 1,
        };
        assert!(validate_response(&invalid).is_err());
    }

    #[test]
    fn peer_credentials_require_exact_uid_not_group_membership() {
        assert!(peer_uid_allowed(1001, 1001));
        assert!(!peer_uid_allowed(1002, 1001));
        assert!(!peer_uid_allowed(0, 1001));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn socket_stale_cleanup_restart_and_instance_isolation() {
        use std::{os::unix::fs::PermissionsExt, time::Duration};
        use tokio::net::UnixListener;

        let root = std::env::temp_dir().join(format!(
            "baft-control-lifecycle-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first_dir = root.join("node-a");
        let second_dir = root.join("node-b");
        std::fs::create_dir_all(&first_dir).unwrap();
        std::fs::create_dir_all(&second_dir).unwrap();
        for dir in [&first_dir, &second_dir] {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o750)).unwrap();
        }
        let first = first_dir.join("control.sock");
        let second = second_dir.join("control.sock");

        // A socket left behind by SIGKILL is removed only inside its validated instance dir.
        let stale = UnixListener::bind(&first).unwrap();
        drop(stale);
        let other = UnixListener::bind(&second).unwrap();
        drop(other);
        let uid = unsafe { libc::geteuid() };
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let server = ControlServer::bind_for_test(first.clone(), uid, &root).unwrap();
        let metadata = std::fs::symlink_metadata(&first).unwrap();
        assert_eq!(metadata.uid(), uid);
        assert_eq!(metadata.gid(), unsafe { libc::getegid() });
        assert_eq!(metadata.mode() & 0o777, 0o660);
        assert_eq!(metadata.mode() & libc::S_IFMT, libc::S_IFSOCK);
        assert!(first.exists());
        assert!(
            second.exists(),
            "another instance's socket must remain untouched"
        );
        let task = tokio::spawn(server.run(
            std::sync::Arc::new(ProbeControl::default()),
            std::sync::Arc::new(ProbeLimiter::default()),
            stop_rx,
        ));
        let response = timeout(Duration::from_secs(2), request(&first))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.result, "unknown");
        assert_eq!(response.reason.as_deref(), Some("peer_unavailable"));
        let _ = stop_tx.send(true);
        timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!first.exists(), "graceful shutdown removes its own socket");
        assert!(
            second.exists(),
            "shutdown must not remove a sibling instance socket"
        );

        // Restart rebinds the same instance path after cleanup.
        let restarted = ControlServer::bind_for_test(first.clone(), uid, &root).unwrap();
        let (stop_tx2, stop_rx2) = tokio::sync::watch::channel(false);
        let task2 = tokio::spawn(restarted.run(
            std::sync::Arc::new(ProbeControl::default()),
            std::sync::Arc::new(ProbeLimiter::default()),
            stop_rx2,
        ));
        let response = timeout(Duration::from_secs(2), request(&first))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.result, "unknown");
        let _ = stop_tx2.send(true);
        timeout(Duration::from_secs(2), task2)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        std::fs::remove_file(second).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn socket_path_refuses_symlinked_runtime_directory_and_non_socket_stale_path() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let root = std::env::temp_dir().join(format!(
            "baft-control-path-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let external = root.join("external");
        let link = root.join("node-link");
        std::fs::create_dir_all(&external).unwrap();
        std::fs::set_permissions(&external, std::fs::Permissions::from_mode(0o750)).unwrap();
        symlink(&external, &link).unwrap();
        let uid = unsafe { libc::geteuid() };
        let gid = unsafe { libc::getegid() };
        assert!(prepare_socket_path_under(&link.join("control.sock"), &root, uid, gid).is_err());

        let node = root.join("node-a");
        std::fs::create_dir(&node).unwrap();
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o750)).unwrap();
        let victim = node.join("control.sock");
        std::fs::write(&victim, b"do not replace").unwrap();
        assert!(prepare_socket_path_under(&victim, &root, uid, gid).is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"do not replace");
        std::fs::remove_file(&victim).unwrap();
        let target = root.join("target");
        std::fs::write(&target, b"preserve target").unwrap();
        let link_path = node.join("control.sock");
        symlink(&target, &link_path).unwrap();
        assert!(prepare_socket_path_under(&link_path, &root, uid, gid).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"preserve target");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn socket_request_accepts_only_exact_kernel_peer_uid() {
        use tokio::net::UnixListener;

        let path = std::env::temp_dir().join(format!(
            "baft-control-uid-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let expected_uid = unsafe { libc::geteuid() };
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve_connection(
                stream,
                expected_uid,
                std::sync::Arc::new(ProbeControl::default()),
                std::sync::Arc::new(ProbeLimiter::default()),
            )
            .await
        });
        let response = request(&path).await.unwrap();
        assert_eq!(response.result, "unknown");
        assert_eq!(response.reason.as_deref(), Some("peer_unavailable"));
        server.await.unwrap().unwrap();
        let _ = std::fs::remove_file(path);

        let path = std::env::temp_dir().join(format!(
            "baft-control-deny-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let wrong_uid = expected_uid.wrapping_add(1);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve_connection(
                stream,
                wrong_uid,
                std::sync::Arc::new(ProbeControl::default()),
                std::sync::Arc::new(ProbeLimiter::default()),
            )
            .await
        });
        assert!(request(&path).await.is_err());
        assert!(server.await.unwrap().is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pass_receipt_requires_exact_payload_digest_flag_and_bounded_duration() {
        let valid = ProbeControlResponse {
            result: "pass".into(),
            reason: None,
            bytes: crate::integrity_probe::PROBE_BYTES,
            sha256_match: true,
            duration_ms: Some(9_999),
            timestamp: 1,
        };
        validate_response(&valid).unwrap();

        let mut missing_timestamp = valid.clone();
        missing_timestamp.timestamp = 0;
        assert!(validate_response(&missing_timestamp).is_err());

        let mut over_deadline = valid;
        over_deadline.duration_ms = Some(10_001);
        assert!(validate_response(&over_deadline).is_err());

        let failure = ProbeControlResponse::from_result(Err(anyhow::anyhow!("SHA-256 mismatch")));
        assert_eq!(failure.result, "fail");
        assert_eq!(failure.reason.as_deref(), Some("integrity_mismatch"));
        validate_response(&failure).unwrap();

        let mut invalid_failure = failure;
        invalid_failure.reason = Some("probe_failed".into());
        assert!(validate_response(&invalid_failure).is_err());
    }

    #[test]
    fn response_contains_only_bounded_non_secret_metadata() {
        let response = ProbeControlResponse::from_result(Err(anyhow::anyhow!("rate limit")));
        assert_eq!(response.result, "unknown");
        assert_eq!(response.reason.as_deref(), Some("rate_limited"));
        assert_eq!(response.bytes, 0);
        assert!(!response.sha256_match);
        let wire = serde_json::to_value(&response).unwrap();
        assert!(wire.get("sha256").is_none());
        assert!(serde_json::to_vec(&response).unwrap().len() < MAX_RESPONSE_BYTES);
    }
}
