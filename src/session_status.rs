//! Observable session indicator (ADR-0022, P0-2) and unit-side operational evidence (ADR-0024, P1).
//!
//! While at least one *authenticated and authorized* session exists, the running unit writes
//! `$RUNTIME_DIRECTORY/session.json`; it is removed when the last session ends. The agent reads it
//! to tell "listener is up" apart from "a peer session was authorized". It says nothing about data
//! integrity: that stays Unknown until a data-path probe observes it.
//!
//! The unit also publishes `counters.json` (cumulative relayed payload bytes per direction, stream
//! and session totals, sampled every [`COUNTER_SECONDS`]) and `last_failure.json` (the stage of
//! the most recent failed connection attempt). None of these files holds secrets, payload,
//! addresses or error text: only ids, counts, stage names, timestamps and the process id.
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

static ACTIVE: Mutex<u32> = Mutex::new(0);
static BYTES_FROM_PEER: AtomicU64 = AtomicU64::new(0);
static BYTES_TO_PEER: AtomicU64 = AtomicU64::new(0);
static STREAMS_TOTAL: AtomicU64 = AtomicU64::new(0);
static SESSIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static PUBLISHER: std::sync::Once = std::sync::Once::new();

pub const FILE: &str = "session.json";
pub const COUNTERS_FILE: &str = "counters.json";
pub const FAILURE_FILE: &str = "last_failure.json";
/// How often the unit rewrites `counters.json`.
pub const COUNTER_SECONDS: u64 = 5;

/// Connection-establishment stage, attached as `anyhow` context where a stage can fail, so a
/// failure is attributed to exactly one evidence tier (ADR-0024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// TCP connect to the peer.
    Tcp,
    /// TLS 1.3 / Noise handshake: certificate chain, CRL, protocol, static key.
    SecureChannel,
    /// Peer certificate pin or URI SAN node identity.
    Identity,
    /// Protocol handshake: node ids, tunnel id and direction authorization.
    Authorization,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::SecureChannel => "secure_channel",
            Self::Identity => "identity",
            Self::Authorization => "authorization",
        }
    }
}

impl std::fmt::Display for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} stage failed", self.as_str())
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn dir() -> Option<PathBuf> {
    std::env::var_os("RUNTIME_DIRECTORY")
        .map(|v| {
            PathBuf::from(
                v.to_string_lossy()
                    .split(':')
                    .next()
                    .unwrap_or("")
                    .to_owned(),
            )
        })
        .filter(|p| p.is_absolute() && p.is_dir())
}

fn write_json(dir: &Path, name: &str, body: &serde_json::Value) {
    use std::os::unix::fs::PermissionsExt;
    let tmp = dir.join(format!(".{name}.tmp"));
    if std::fs::write(&tmp, body.to_string()).is_ok() {
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644));
        let _ = std::fs::rename(&tmp, dir.join(name));
    }
}

fn write(dir: &Path, tunnel: &str, peer: &str, direction: &str, count: u32) {
    let body = serde_json::json!({
        "state": "authorized",
        "authorized_sessions": count,
        "tunnel_id": tunnel,
        "peer_id": peer,
        "direction": direction,
        "pid": std::process::id(),
        "since_unix": now(),
    });
    write_json(dir, FILE, &body);
}

/// Count payload bytes relayed from the peer (tunnel → local side).
pub fn add_from_peer(n: usize) {
    BYTES_FROM_PEER.fetch_add(n as u64, Ordering::Relaxed);
}

/// Count payload bytes relayed to the peer (local side → tunnel).
pub fn add_to_peer(n: usize) {
    BYTES_TO_PEER.fetch_add(n as u64, Ordering::Relaxed);
}

/// Count one authorized multiplexed stream.
pub fn stream_opened() {
    STREAMS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

fn counters_body(started: u64) -> serde_json::Value {
    serde_json::json!({
        "counters_version": 1,
        "pid": std::process::id(),
        "started_unix": started,
        "sampled_unix": now(),
        "interval_s": COUNTER_SECONDS,
        "units": "bytes",
        "bytes_from_peer": BYTES_FROM_PEER.load(Ordering::Relaxed),
        "bytes_to_peer": BYTES_TO_PEER.load(Ordering::Relaxed),
        "streams_total": STREAMS_TOTAL.load(Ordering::Relaxed),
        "sessions_total": SESSIONS_TOTAL.load(Ordering::Relaxed),
    })
}

/// Start (once per process) the task that rewrites `counters.json`. A no-op outside a systemd
/// unit (no `RUNTIME_DIRECTORY`). Must be called inside a Tokio runtime.
pub fn start_publisher() {
    let Some(d) = dir() else { return };
    PUBLISHER.call_once(|| {
        let started = now();
        write_json(&d, COUNTERS_FILE, &counters_body(started));
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(COUNTER_SECONDS));
            loop {
                tick.tick().await;
                write_json(&d, COUNTERS_FILE, &counters_body(started));
            }
        });
    });
}

/// Record the stage of a failed connection attempt, if the error carries one. Only the stage
/// name, time and process id are written; the error text (which may hold addresses) is not.
pub fn record_failure(error: &anyhow::Error) {
    let Some(stage) = error.downcast_ref::<Stage>() else {
        return;
    };
    if let Some(d) = dir() {
        write_json(
            &d,
            FAILURE_FILE,
            &serde_json::json!({"stage": stage.as_str(), "at_unix": now(), "pid": std::process::id()}),
        );
    }
}

/// Held for the lifetime of one authorized session.
pub struct SessionMark {
    tunnel: String,
    peer: String,
    direction: String,
}

impl SessionMark {
    pub fn open(tunnel: &str, peer: &str, direction: impl std::fmt::Debug) -> Self {
        let direction = format!("{direction:?}").to_lowercase();
        SESSIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
        let mut n = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        *n += 1;
        if let Some(d) = dir() {
            write(&d, tunnel, peer, &direction, *n);
        }
        Self {
            tunnel: tunnel.to_owned(),
            peer: peer.to_owned(),
            direction,
        }
    }
}

impl Drop for SessionMark {
    fn drop(&mut self) {
        let mut n = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        *n = n.saturating_sub(1);
        if let Some(d) = dir() {
            if *n == 0 {
                let _ = std::fs::remove_file(d.join(FILE));
            } else {
                write(&d, &self.tunnel, &self.peer, &self.direction, *n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_open_and_clear_the_file_and_failures_carry_only_the_stage() {
        let d = std::env::temp_dir().join(format!("baft-sess-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // SAFETY: single test touching this variable in this module.
        unsafe { std::env::set_var("RUNTIME_DIRECTORY", &d) };
        let a = SessionMark::open("t1", "abroad", "Direct");
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(d.join(FILE)).unwrap()).unwrap();
        assert_eq!(v["state"], "authorized");
        assert_eq!(v["authorized_sessions"], 1);
        assert_eq!(v["peer_id"], "abroad");
        assert_eq!(v["pid"], std::process::id());
        let b = SessionMark::open("t1", "abroad", "Direct");
        drop(a);
        assert!(d.join(FILE).exists());
        drop(b);
        assert!(!d.join(FILE).exists());

        let staged = anyhow::anyhow!("handshake with 192.0.2.7 failed").context(Stage::Identity);
        record_failure(&staged.context("outer"));
        let text = std::fs::read_to_string(d.join(FAILURE_FILE)).unwrap();
        let f: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(f["stage"], "identity");
        assert!(!text.contains("192.0.2.7"), "{text}");
        std::fs::remove_file(d.join(FAILURE_FILE)).unwrap();
        record_failure(&anyhow::anyhow!("no stage"));
        assert!(!d.join(FAILURE_FILE).exists());

        add_from_peer(10);
        add_to_peer(3);
        stream_opened();
        let c = counters_body(1);
        assert!(c["bytes_from_peer"].as_u64().unwrap() >= 10);
        assert!(c["bytes_to_peer"].as_u64().unwrap() >= 3);
        assert_eq!(c["units"], "bytes");
        unsafe { std::env::remove_var("RUNTIME_DIRECTORY") };
    }
}
