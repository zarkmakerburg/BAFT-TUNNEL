//! Generic managed-runtime host (ADR-0020).
//!
//! The Rust core starts an independent engine binary (Go today, any language later) as a
//! child process. This module knows nothing about any engine's protocol or configuration:
//! it verifies a pinned artifact, runs a validate step and the engine, probes readiness,
//! restarts within a bound, and stops the child gracefully. Tunnel payload never passes
//! through here. Each engine supplies its own adapter that builds a [`ChildSpec`].

use std::collections::BTreeSet;
use std::fs;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::process::{Child, Command};
use tokio::time::Instant;

/// The only manifest schema this build understands.
pub const MANIFEST_SCHEMA: u32 = 1;

/// One entry of the versioned engine runtime manifest (`release/engine-manifest.json`).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EngineRuntime {
    pub id: String,
    pub runtime_language: String,
    pub version: String,
    pub source_repository: String,
    pub source_commit: String,
    pub artifact: String,
    pub artifact_sha256: String,
    pub platforms: Vec<String>,
    pub config_adapter: String,
    pub required_capabilities: Vec<String>,
    pub readiness: ReadinessContract,
    pub resources: ResourcePolicy,
    pub license: LicenseInfo,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub enum ReadinessContract {
    /// Ready when the local listen address accepts a TCP connection.
    TcpListen { timeout_seconds: u64 },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResourcePolicy {
    pub max_restarts: u32,
    pub restart_window_seconds: u64,
    pub max_memory_mib: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LicenseInfo {
    pub spdx: String,
    pub source_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub engines: Vec<EngineRuntime>,
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Manifest {
    /// Parse and strictly validate a manifest. Unknown fields, duplicate ids, unpinned
    /// sources and malformed hashes are all rejected.
    pub fn parse(text: &str) -> Result<Self> {
        let manifest: Self = serde_json::from_str(text).context("parse engine manifest")?;
        if manifest.schema_version != MANIFEST_SCHEMA {
            bail!(
                "unsupported engine manifest schema {}, expected {MANIFEST_SCHEMA}",
                manifest.schema_version
            );
        }
        let mut seen = BTreeSet::new();
        for engine in &manifest.engines {
            engine.validate()?;
            if !seen.insert(engine.id.clone()) {
                bail!("duplicate engine id {}", engine.id);
            }
        }
        Ok(manifest)
    }

    pub fn engine(&self, id: &str) -> Option<&EngineRuntime> {
        self.engines.iter().find(|engine| engine.id == id)
    }
}

impl EngineRuntime {
    fn validate(&self) -> Result<()> {
        let id = &self.id;
        if id.is_empty()
            || id.len() > 32
            || !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            bail!("engine id {id:?} must be 1-32 chars of a-z, 0-9 and '-'");
        }
        if !is_commit(&self.source_commit) {
            bail!("engine {id}: source_commit must be a full 40-hex commit");
        }
        if !is_sha256(&self.artifact_sha256) {
            bail!("engine {id}: artifact_sha256 must be 64 hex chars");
        }
        if !self.source_repository.starts_with("https://") {
            bail!("engine {id}: source_repository must be an https URL");
        }
        if self.artifact.is_empty()
            || self.artifact.contains("..")
            || self.artifact.starts_with('/')
        {
            bail!("engine {id}: artifact must be a relative path inside the release");
        }
        if self.platforms.is_empty() {
            bail!("engine {id}: at least one platform is required");
        }
        if self.version.is_empty()
            || self.license.spdx.is_empty()
            || self.license.source_url.is_empty()
        {
            bail!("engine {id}: version and license metadata are required");
        }
        if self.resources.max_restarts > 20 || self.resources.restart_window_seconds == 0 {
            bail!("engine {id}: restart policy out of range");
        }
        let ReadinessContract::TcpListen { timeout_seconds } = &self.readiness;
        if *timeout_seconds == 0 || *timeout_seconds > 600 {
            bail!("engine {id}: readiness timeout must be 1..=600 seconds");
        }
        Ok(())
    }
}

/// Refuse an artifact whose SHA-256 differs from the pinned value, or that others can write.
pub fn verify_artifact(path: &Path, sha256: &str) -> Result<()> {
    let meta = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if !meta.is_file() || meta.permissions().mode() & 0o022 != 0 {
        bail!("engine binary must be a regular file not writable by group or other");
    }
    let digest = hex::encode(Sha256::digest(
        fs::read(path).context("read engine binary")?,
    ));
    if !digest.eq_ignore_ascii_case(sha256) {
        bail!("engine binary SHA-256 does not match the pinned value; refusing to start it");
    }
    Ok(())
}

/// How to start one engine. Built by the engine's adapter; opaque to the host.
#[derive(Debug, Clone)]
pub struct ChildSpec {
    pub binary: PathBuf,
    /// Run once before the engine starts; a failure stops the start.
    pub validate_args: Option<Vec<String>>,
    pub run_args: Vec<String>,
    /// Address that accepts TCP once the engine is serving (local listener).
    pub ready_addr: SocketAddr,
    pub ready_timeout: Duration,
    pub max_restarts: u32,
    pub restart_window: Duration,
    pub grace: Duration,
}

/// Observable state, for logs and later for the agent status endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Ready,
    Restarting,
    Stopped,
    Failed,
}

fn command(spec: &ChildSpec, args: &[String]) -> Command {
    let mut cmd = Command::new(&spec.binary);
    cmd.args(args)
        .env_clear()
        .stdin(Stdio::null())
        .kill_on_drop(true);
    cmd
}

async fn wait_ready(spec: &ChildSpec, child: &mut Child) -> Result<()> {
    let deadline = Instant::now() + spec.ready_timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            bail!("engine exited before becoming ready: {status}");
        }
        if tokio::net::TcpStream::connect(spec.ready_addr)
            .await
            .is_ok()
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "engine did not accept connections on {} within {}s",
                spec.ready_addr,
                spec.ready_timeout.as_secs()
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn stop(child: &mut Child, grace: Duration) {
    if let Some(pid) = child.id() {
        // SAFETY: plain kill(2) on the pid of a child this function owns.
        unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    }
    if tokio::time::timeout(grace, child.wait()).await.is_err() {
        child.kill().await.ok();
    }
}

/// Validate, start, probe readiness and keep the engine running until `shutdown` resolves.
/// An exit is restarted with backoff at most `max_restarts` times per `restart_window`;
/// beyond that the host fails so the service manager sees the tunnel as down. A running
/// process is reported ready only after the readiness probe succeeds.
pub async fn supervise(
    spec: &ChildSpec,
    shutdown: impl std::future::Future<Output = ()>,
    mut on_phase: impl FnMut(Phase),
) -> Result<()> {
    on_phase(Phase::Starting);
    if let Some(args) = &spec.validate_args {
        let status = command(spec, args)
            .status()
            .await
            .context("run engine validate step")?;
        if !status.success() {
            on_phase(Phase::Failed);
            bail!("engine rejected its configuration: {status}");
        }
    }
    tokio::pin!(shutdown);
    let mut restarts: Vec<Instant> = Vec::new();
    loop {
        let mut child = command(spec, &spec.run_args)
            .spawn()
            .context("start engine")?;
        match wait_ready(spec, &mut child).await {
            Ok(()) => on_phase(Phase::Ready),
            Err(error) => {
                stop(&mut child, spec.grace).await;
                on_phase(Phase::Failed);
                return Err(error);
            }
        }
        tokio::select! {
            status = child.wait() => {
                let status = status?;
                let now = Instant::now();
                restarts.retain(|at| now.duration_since(*at) < spec.restart_window);
                if restarts.len() as u32 >= spec.max_restarts {
                    on_phase(Phase::Failed);
                    bail!("engine exited ({status}) and the restart budget is spent");
                }
                restarts.push(now);
                on_phase(Phase::Restarting);
                let backoff = Duration::from_secs(1 << restarts.len().min(4));
                tokio::select! {
                    () = tokio::time::sleep(backoff) => {},
                    () = &mut shutdown => { on_phase(Phase::Stopped); return Ok(()); }
                }
            }
            () = &mut shutdown => {
                stop(&mut child, spec.grace).await;
                on_phase(Phase::Stopped);
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> String {
        r#"{"id":"baft","runtime_language":"go","version":"0.5.0",
        "source_repository":"https://github.com/zarkmakerburg/BAFT",
        "source_commit":"2c8e14efdecabff392f061b2ea9538808597f2a2",
        "artifact":"engines/baft-linux-amd64",
        "artifact_sha256":"0000000000000000000000000000000000000000000000000000000000000000",
        "platforms":["linux-amd64"],"config_adapter":"baft-v1",
        "required_capabilities":["mtls"],
        "readiness":{"kind":"tcp_listen","timeout_seconds":20},
        "resources":{"max_restarts":3,"restart_window_seconds":300,"max_memory_mib":512},
        "license":{"spdx":"AGPL-3.0-only","source_url":"https://github.com/zarkmakerburg/BAFT"}}"#
            .to_string()
    }

    fn manifest(engine: &str) -> String {
        format!(r#"{{"schema_version":1,"engines":[{engine}]}}"#)
    }

    #[test]
    fn parses_valid_manifest() {
        let parsed = Manifest::parse(&manifest(&entry())).unwrap();
        assert_eq!(parsed.engine("baft").unwrap().runtime_language, "go");
    }

    #[test]
    fn rejects_bad_manifests() {
        let bad = [
            manifest(&entry().replace("2c8e14efdecabff392f061b2ea9538808597f2a2", "main")),
            manifest(&entry().replace("\"engines/baft-linux-amd64\"", "\"/usr/bin/x\"")),
            manifest(&entry().replace("\"engines/baft-linux-amd64\"", "\"../x\"")),
            manifest(&entry().replace("00000000000000000000000000000000", "zz")),
            manifest(&entry().replace("\"id\":\"baft\"", "\"id\":\"BAFT\"")),
            manifest(&entry().replace("\"timeout_seconds\":20", "\"timeout_seconds\":0")),
            manifest(&entry().replace("\"max_restarts\":3", "\"max_restarts\":99")),
            manifest(&entry().replace("\"platforms\":[\"linux-amd64\"]", "\"platforms\":[]")),
            manifest(&entry().replace("\"go\"", "\"go\",\"extra\":1")),
            manifest(&format!("{},{}", entry(), entry())),
            manifest(&entry()).replace("\"schema_version\":1", "\"schema_version\":2"),
        ];
        for text in bad {
            assert!(Manifest::parse(&text).is_err(), "accepted: {text}");
        }
    }
}

#[cfg(test)]
mod supervise_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::TcpListener;

    fn script(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rh-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("engine");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn spec(binary: PathBuf, addr: SocketAddr, max_restarts: u32) -> ChildSpec {
        ChildSpec {
            binary,
            validate_args: Some(vec!["validate".into()]),
            run_args: vec!["run".into()],
            ready_addr: addr,
            ready_timeout: Duration::from_secs(3),
            max_restarts,
            restart_window: Duration::from_secs(60),
            grace: Duration::from_secs(5),
        }
    }

    #[test]
    fn artifact_hash_and_mode_are_enforced() {
        let path = script("hash", "exit 0");
        let good = hex::encode(Sha256::digest(fs::read(&path).unwrap()));
        verify_artifact(&path, &good).unwrap();
        assert!(verify_artifact(&path, &"0".repeat(64)).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(verify_artifact(&path, &good).is_err());
    }

    #[tokio::test]
    async fn ready_then_graceful_stop() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let bin = script(
            "ok",
            "[ \"$1\" = validate ] && exit 0\ntrap 'exit 0' TERM\nwhile true; do sleep 1; done",
        );
        let mut phases = Vec::new();
        let started = std::time::Instant::now();
        supervise(
            &spec(bin, addr, 0),
            tokio::time::sleep(Duration::from_millis(600)),
            |p| phases.push(p),
        )
        .await
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_eq!(phases, [Phase::Starting, Phase::Ready, Phase::Stopped]);
    }

    #[tokio::test]
    async fn rejected_validation_never_starts_the_engine() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bin = script("reject", "exit 3");
        let mut phases = Vec::new();
        let error = supervise(
            &spec(bin, listener.local_addr().unwrap(), 3),
            std::future::pending(),
            |p| phases.push(p),
        )
        .await
        .unwrap_err();
        assert!(format!("{error:#}").contains("rejected its configuration"));
        assert_eq!(phases, [Phase::Starting, Phase::Failed]);
    }

    #[tokio::test]
    async fn running_process_is_not_ready_until_the_probe_passes() {
        // Nothing listens on the probe address, although the process stays alive.
        let free = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap()
        };
        let bin = script(
            "silent",
            "[ \"$1\" = validate ] && exit 0\nwhile true; do sleep 1; done",
        );
        let mut phases = Vec::new();
        let mut s = spec(bin, free, 3);
        s.ready_timeout = Duration::from_secs(1);
        s.grace = Duration::from_secs(1);
        let error = supervise(&s, std::future::pending(), |p| phases.push(p))
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("did not accept connections"));
        assert!(!phases.contains(&Phase::Ready));
    }

    #[tokio::test]
    async fn restart_budget_is_bounded() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // Becomes ready (listener above), then exits at once, every time.
        let bin = script("flap", "[ \"$1\" = validate ] && exit 0\nsleep 0.3\nexit 9");
        let mut phases = Vec::new();
        let error = supervise(&spec(bin, addr, 2), std::future::pending(), |p| {
            phases.push(p)
        })
        .await
        .unwrap_err();
        assert!(format!("{error:#}").contains("restart budget is spent"));
        assert_eq!(
            phases.iter().filter(|p| **p == Phase::Restarting).count(),
            2
        );
        assert_eq!(phases.iter().filter(|p| **p == Phase::Ready).count(), 3);
    }
}
