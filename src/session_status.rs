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
struct HealthState {
    tracker: crate::transport_health::HealthTracker,
    feed: crate::transport_health::LinkFeed,
}
static HEALTH: Mutex<Option<HealthState>> = Mutex::new(None);
/// The most recent carrier socket (fd and the peer it was connected to), for kernel TCP samples.
static CARRIER: Mutex<Option<(i32, std::net::SocketAddr)>> = Mutex::new(None);
static ROTATIONS: AtomicU64 = AtomicU64::new(0);
static LINK_SWITCHES: AtomicU64 = AtomicU64::new(0);
static LAST_ROTATION: AtomicU64 = AtomicU64::new(0);
static LAST_SWITCH: AtomicU64 = AtomicU64::new(0);
static PUBLISHER: std::sync::Once = std::sync::Once::new();

pub const FILE: &str = "session.json";
pub const COUNTERS_FILE: &str = "counters.json";
pub const FAILURE_FILE: &str = "last_failure.json";
/// Transport health (ADR-0041), separate from Integrity (ADR-0028).
pub const HEALTH_FILE: &str = "health.json";
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

/// The systemd-provided runtime directory, when this process runs inside the unit. Shared with
/// `policy_status`, which publishes the ADR-0047 running snapshot the same way.
pub(crate) fn runtime_dir() -> Option<PathBuf> {
    dir()
}

/// Atomically publish one evidence file into the unit's runtime directory.
pub(crate) fn publish_json(dir: &Path, name: &str, body: &serde_json::Value) {
    write_json(dir, name, body);
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

/// A make-before-break session rotation completed (ADR-0049). Counted as an event; it does not
/// by itself say anything about link quality.
pub fn rotation_completed() {
    ROTATIONS.fetch_add(1, Ordering::Relaxed);
    LAST_ROTATION.store(now(), Ordering::Relaxed);
}

/// The dialing loop moved to a different configured link (failover or failback, ADR-0039).
pub fn link_switched() {
    LINK_SWITCHES.fetch_add(1, Ordering::Relaxed);
    LAST_SWITCH.store(now(), Ordering::Relaxed);
}

fn events_body() -> serde_json::Value {
    let opt = |v: u64| (v > 0).then_some(v);
    serde_json::json!({
        "rotations_total": ROTATIONS.load(Ordering::Relaxed),
        "link_switches_total": LINK_SWITCHES.load(Ordering::Relaxed),
        "last_rotation_unix": opt(LAST_ROTATION.load(Ordering::Relaxed)),
        "last_link_switch_unix": opt(LAST_SWITCH.load(Ordering::Relaxed)),
    })
}

/// Remember the newest carrier socket so the sampler can read its kernel TCP state. Called when a
/// tunnel socket is tuned. The fd is only ever read (`TCP_INFO`), never written or closed, and a
/// sample is accepted only while `getpeername` still returns the recorded peer, so a closed and
/// reused descriptor is detected instead of misread.
pub fn register_carrier(fd: i32, peer: std::net::SocketAddr) {
    *CARRIER.lock().unwrap_or_else(|e| e.into_inner()) = Some((fd, peer));
    if let Some(h) = HEALTH.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        h.feed.reset();
    }
}

fn clear_carrier() {
    *CARRIER.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[cfg(target_os = "linux")]
fn read_tcp_info(
    fd: i32,
    peer: std::net::SocketAddr,
) -> Option<crate::transport_health::LinkSample> {
    use std::time::Duration;
    // SAFETY: zeroed sockaddr_storage/tcp_info are valid plain-data buffers; lengths are passed
    // by pointer with their exact sizes; the calls only read socket state.
    unsafe {
        let mut ss: libc::sockaddr_storage = std::mem::zeroed();
        let mut sl = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if libc::getpeername(fd, (&mut ss as *mut libc::sockaddr_storage).cast(), &mut sl) != 0 {
            return None;
        }
        let actual = match ss.ss_family as libc::c_int {
            libc::AF_INET => {
                let a = &*(&ss as *const libc::sockaddr_storage).cast::<libc::sockaddr_in>();
                std::net::SocketAddr::from((
                    std::net::Ipv4Addr::from(u32::from_be(a.sin_addr.s_addr)),
                    u16::from_be(a.sin_port),
                ))
            }
            libc::AF_INET6 => {
                let a = &*(&ss as *const libc::sockaddr_storage).cast::<libc::sockaddr_in6>();
                std::net::SocketAddr::from((
                    std::net::Ipv6Addr::from(a.sin6_addr.s6_addr),
                    u16::from_be(a.sin6_port),
                ))
            }
            _ => return None,
        };
        if actual != peer {
            return None;
        }
        let mut info: libc::tcp_info = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::tcp_info>() as libc::socklen_t;
        if libc::getsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_INFO,
            (&mut info as *mut libc::tcp_info).cast(),
            &mut len,
        ) != 0
        {
            return None;
        }
        // An older kernel returns a shorter struct; the fields used here need the newer layout.
        if (len as usize) < std::mem::offset_of!(libc::tcp_info, tcpi_busy_time) + 8 {
            return None;
        }
        Some(crate::transport_health::LinkSample {
            at: Duration::from_secs(now()),
            rtt: Duration::from_micros(u64::from(info.tcpi_rtt)),
            retrans_total: info.tcpi_total_retrans,
            bytes_acked: info.tcpi_bytes_acked,
            busy: Duration::from_micros(info.tcpi_busy_time),
        })
    }
}

#[cfg(not(target_os = "linux"))]
fn read_tcp_info(_: i32, _: std::net::SocketAddr) -> Option<crate::transport_health::LinkSample> {
    None
}

/// Take one kernel sample of the current carrier and feed the health model. No carrier, no
/// session or an unreadable socket feeds nothing, so quality stays Unknown.
fn sample_link() {
    let active = *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) > 0;
    let carrier = *CARRIER.lock().unwrap_or_else(|e| e.into_inner());
    let (true, Some((fd, peer))) = (active, carrier) else {
        return;
    };
    let Some(sample) = read_tcp_info(fd, peer) else {
        return;
    };
    let mut g = HEALTH.lock().unwrap_or_else(|e| e.into_inner());
    let h = g.get_or_insert_with(new_health);
    h.feed.observe(sample);
}

fn new_health() -> HealthState {
    HealthState {
        tracker: crate::transport_health::HealthTracker::new(
            crate::transport_health::Thresholds::default(),
        ),
        feed: crate::transport_health::LinkFeed::new(),
    }
}

/// Health of the link as the unit can honestly state it. Quality comes only from kernel TCP
/// samples of the carrier socket (smoothed RTT, retransmitting intervals, delivery rate while
/// busy); with none, an authorized session reports `authenticated` and the numbers are null.
/// Not Integrity.
fn health_body(started: u64) -> serde_json::Value {
    use crate::transport_health::{HealthState as S, Phase};
    let active = *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) > 0;
    let phase = if active {
        Phase::Authenticated
    } else if SESSIONS_TOTAL.load(Ordering::Relaxed) > 0 {
        Phase::Down
    } else {
        Phase::Connecting
    };
    let mut guard = HEALTH.lock().unwrap_or_else(|e| e.into_inner());
    let h = guard.get_or_insert_with(new_health);
    let elapsed = std::time::Duration::from_secs(now().saturating_sub(started));
    let throttled = h.feed.watch.throttled_rate();
    let r = h
        .tracker
        .evaluate_with(elapsed, phase, &h.feed.rtt, &h.feed.probes, None, throttled);
    let state = match r.state {
        S::Connecting => "connecting",
        S::Connected => "connected",
        S::Authenticated => "authenticated",
        S::Healthy => "healthy",
        S::Degraded => "degraded",
        S::Recovering => "recovering",
        S::Unavailable => "unavailable",
    };
    serde_json::json!({
        "health_version": 1,
        "pid": std::process::id(),
        "sampled_unix": now(),
        "state": state,
        "reasons": r.reasons,
        "rtt_ms": r.rtt_ms,
        "jitter_ms": r.jitter_ms,
        "probe_loss": r.probe_loss,
        "loss_basis": "share of sampling intervals with TCP retransmissions",
        "rtt_basis": "kernel smoothed RTT of the carrier socket",
        "throttled_rate_bps": throttled,
        "events": events_body(),
    })
}

/// Start (once per process) the task that rewrites `counters.json`. A no-op outside a systemd
/// unit (no `RUNTIME_DIRECTORY`). Must be called inside a Tokio runtime.
pub fn start_publisher() {
    let Some(d) = dir() else { return };
    PUBLISHER.call_once(|| {
        let started = now();
        write_json(&d, COUNTERS_FILE, &counters_body(started));
        write_json(&d, HEALTH_FILE, &health_body(started));
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(COUNTER_SECONDS));
            loop {
                tick.tick().await;
                sample_link();
                write_json(&d, COUNTERS_FILE, &counters_body(started));
                write_json(&d, HEALTH_FILE, &health_body(started));
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
        if *n == 0 {
            clear_carrier();
        }
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
        // Health: no session left -> unavailable; with a session -> authenticated and quality
        // stays unknown (null), never healthy without samples.
        let down = health_body(1);
        assert_eq!(down["state"], "unavailable");
        let live = SessionMark::open("t1", "abroad", "Direct");
        let up = health_body(1);
        assert_eq!(up["state"], "authenticated");
        assert!(up["rtt_ms"].is_null() && up["jitter_ms"].is_null() && up["probe_loss"].is_null());
        drop(live);
        // Events are counted and timestamped; they are not a quality signal.
        rotation_completed();
        link_switched();
        let ev = health_body(1)["events"].clone();
        assert!(ev["rotations_total"].as_u64().unwrap() >= 1);
        assert!(ev["link_switches_total"].as_u64().unwrap() >= 1);
        assert!(ev["last_rotation_unix"].is_u64() && ev["last_link_switch_unix"].is_u64());
        unsafe { std::env::remove_var("RUNTIME_DIRECTORY") };
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn tcp_info_is_read_only_while_the_peer_matches() {
        use std::os::fd::AsRawFd;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let c = std::net::TcpStream::connect(addr).unwrap();
        let (_s, _) = l.accept().unwrap();
        let sample = read_tcp_info(c.as_raw_fd(), addr).expect("sample");
        assert!(sample.busy >= std::time::Duration::ZERO);
        // A different recorded peer (a reused descriptor) must not be sampled.
        let other: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
        assert!(read_tcp_info(c.as_raw_fd(), other).is_none());
        // An invalid descriptor yields nothing.
        assert!(read_tcp_info(-1, addr).is_none());
    }
}
