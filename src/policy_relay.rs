//! Relay wiring for the Adaptive Traffic Policy Pack (ADR-0048 decisions 4 and 5).
//!
//! Compiled only with the `policy-pack` feature. With the feature off, or with the runtime switch
//! unset, `tunnel::yamux_config` keeps the product's 4 MiB per-stream window and the relay keeps
//! today's fixed 16 KiB pump, so the data path is unchanged (ADR-0031 decision 13, ADR-0044
//! decision 6). `tunnel::relay_stream` picks between both paths and has no other mode.
//!
//! # Switch
//!
//! `BAFT_TUNNEL_POLICY_PACK` (see [`ENV_POLICY`]):
//!
//! | value | effect |
//! |---|---|
//! | unset, empty, `off` | pack off; today's data path, byte for byte |
//! | `window-cap[:<bytes>]` | only the per-stream window bound (decision 4) |
//! | `sched[:<quantum>[:<cap>]]` | window bound **and** the per-direction DRR scheduler |
//!
//! `sched` uses the 256 KiB window, a 16 KiB quantum and a 64 KiB per-flow queue cap. Under `sched`
//! each relay direction gets one [`Arbiter`] per session; the relay asks it before every write, so
//! one flow's turn is decided by rotation order rather than by whichever task reached the socket
//! first.
//!
//! # Why the window is the bound, and the scheduler is fairness on top
//!
//! The experiments (`src/policy_baseline.rs`, evidence `simulated`) chose the Yamux per-stream
//! window as the in-flight bound: a relay-level byte budget gated on `send_window()` cannot be
//! credited below `max_recv_window / 2`, so it either stalls or bounds nothing. 256 KiB cut the
//! loaded ping p50 from about 1.9 s to about 98 ms at a 2.5 MB/s bottleneck. The scheduler does not
//! bound bytes in flight; it bounds *who goes next* when several flows share the link, which the
//! window alone cannot do. Its measurement (TP-2.2) is recorded in ADR-0048 as `simulated`.

use std::io;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;

use crate::policy::{Direction, FlowId};
use crate::policy_adaptive::{Adaptation, Counters, DirectionChange, SAMPLE_PERIOD};
use crate::policy_profile::PolicyProfile;
use crate::policy_sched::{
    Arbiter, ArbiterStats, DEFAULT_QUANTUM_BYTES, FlowHandle, SchedulerConfig,
};

/// Runtime switch read by `tunnel::yamux_config` and `tunnel::policy_session`.
pub(crate) const ENV_POLICY: &str = "BAFT_TUNNEL_POLICY_PACK";

/// Per-stream window when the switch is on without a size: Yamux's initial window, about 100 ms of
/// queue in front of a 2.5 MB/s bottleneck.
pub(crate) const DEFAULT_STREAM_WINDOW: u32 = 256 * 1024;

/// Largest per-flow queue cap the switch accepts; above this a single flow could hold more than the
/// product window and the bound would stop meaning anything.
const MAX_QUEUE_CAP_BYTES: usize = 1024 * 1024;

/// The pack's relay configuration. The value comes from configuration, never from payload
/// inspection (ADR-0031 decision 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PolicyRelay {
    /// Yamux `max_stream_window_size` for both session ends.
    pub stream_window: u32,
    /// `Some` enables the per-direction DRR scheduler with these queue bounds.
    pub scheduler: Option<SchedulerConfig>,
}

impl PolicyRelay {
    /// Reads [`ENV_POLICY`] and, when the switch enables the pack, resolves the tunnel's profile
    /// (ADR-0031 decision 19). Absent, empty or `off` keeps the pack off whatever the profile; with
    /// no profile this is exactly the switch's own numbers, so the default path is unchanged.
    pub(crate) fn from_env_and_profile(profile: Option<PolicyProfile>) -> Option<Self> {
        let value = std::env::var(ENV_POLICY).ok()?;
        Self::resolve(&value, profile)
    }

    /// Pure form of [`Self::from_env_and_profile`]: the switch decides *whether* the pack runs, a
    /// profile then supplies the ratified window, quantum and cap. `None` keeps the pack off.
    pub(crate) fn resolve(switch: &str, profile: Option<PolicyProfile>) -> Option<Self> {
        // The switch decides on/off; with no profile its numbers are the whole answer, exactly as
        // before TP-3.1. With a profile its numeric arguments are ignored.
        let from_switch = Self::parse(switch)?;
        let Some(profile) = profile else {
            return Some(from_switch);
        };
        // A profile's numbers are the ADR-0031 table, which
        // `profile_numbers_stay_inside_the_relay_bounds` holds to the same bounds as the switch.
        Some(Self {
            stream_window: profile.window_bytes(),
            scheduler: Some(SchedulerConfig {
                quantum_bytes: profile.quantum_bytes(),
                max_queued_bytes: profile.max_queued_bytes(),
            }),
        })
    }

    /// `off` (or an empty value) disables the pack; `window-cap[:<bytes>]` enables only the window
    /// bound; `sched[:<quantum>[:<cap>]]` enables the window bound and the scheduler. Sizes below
    /// Yamux's initial window (256 KiB) are refused, because both ends assume that window at stream
    /// creation. Anything else is refused rather than guessed, and the relay stays as it is today.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("off") {
            return None;
        }
        let (name, argument) = match value.split_once(':') {
            Some((name, argument)) => (name, Some(argument)),
            None => (value, None),
        };
        let window_cap = |argument: Option<&str>| -> Option<u32> {
            match argument {
                None => Some(DEFAULT_STREAM_WINDOW),
                Some(argument) => {
                    let window: u32 = argument.trim().parse().ok()?;
                    if window < tokio_yamux::config::INITIAL_STREAM_WINDOW {
                        return None;
                    }
                    Some(window)
                }
            }
        };
        if name.eq_ignore_ascii_case("window-cap") {
            return Some(Self {
                stream_window: window_cap(argument)?,
                scheduler: None,
            });
        }
        if !name.eq_ignore_ascii_case("sched") {
            return None;
        }
        let stream_window = window_cap(None)?;
        let mut parts = argument
            .into_iter()
            .flat_map(|argument| argument.split(':'));
        let quantum = match parts.next() {
            None => DEFAULT_QUANTUM_BYTES,
            Some(argument) => argument.trim().parse::<usize>().ok()?,
        };
        let cap = match parts.next() {
            None => (quantum * 4).clamp(quantum, MAX_QUEUE_CAP_BYTES),
            Some(argument) => argument.trim().parse::<usize>().ok()?,
        };
        if parts.next().is_some() {
            return None;
        }
        // One chunk in flight per flow means a queue never exceeds the quantum; a quantum above the
        // cap would make the scheduler refuse every request, so the pair is validated here.
        if quantum < 1024 || quantum > DEFAULT_STREAM_WINDOW as usize || cap < quantum {
            return None;
        }
        if cap > MAX_QUEUE_CAP_BYTES {
            return None;
        }
        Some(Self {
            stream_window,
            scheduler: Some(SchedulerConfig {
                quantum_bytes: quantum,
                max_queued_bytes: cap,
            }),
        })
    }
}

/// One tunnel session's scheduling state: one arbiter per relay direction and the flow ids that
/// name a stream in both of them. Created by `tunnel`, never by payload.
#[derive(Debug)]
pub(crate) struct PolicySession {
    to_peer: Arc<Arbiter>,
    from_peer: Arc<Arbiter>,
    /// Read size per direction. It follows that direction's adaptation down the ladder, so a
    /// smaller quantum never meets a request larger than the row's queue cap (ADR-0056 decision 2).
    chunk: [AtomicUsize; 2],
    next_flow: AtomicU64,
    /// `Some` only when `policy.adaptive` opted in (ADR-0056 decisions 9-10).
    adaptation: Option<Mutex<Adaptation>>,
}

impl PolicySession {
    /// `Some` only when the switch enables the pack and a scheduler applies; a `window-cap` switch
    /// with no configured profile keeps today's unscheduled path (ADR-0048). `adaptive` is the
    /// tunnel config's `policy.adaptive` opt-in: with no configured profile it stays inert, because
    /// the loop selects rows and never enables a profile (ADR-0056 decisions 1 and 10).
    pub(crate) fn from_env_and_profile(
        profile: Option<PolicyProfile>,
        adaptive: bool,
    ) -> Option<Arc<Self>> {
        let relay = PolicyRelay::from_env_and_profile(profile)?;
        let config = relay.scheduler?;
        let Some(profile) = profile else {
            if adaptive {
                eprintln!(
                    "policy adaptive: inert (policy.profile is absent, so there is no row to adapt from)"
                );
            }
            return Some(Self::with_config(config));
        };
        Some(Self::new(config, profile, adaptive))
    }

    /// Session with fixed bounds, for the measurement harness.
    pub(crate) fn with_config(config: SchedulerConfig) -> Arc<Self> {
        Self::new(config, PolicyProfile::Balanced, false)
    }

    /// Session that starts at `configured` and, when `adaptive`, samples the two arbiters at the
    /// ADR's 1 Hz cadence and applies the controller's chosen row to the direction that caused it.
    pub(crate) fn new(
        config: SchedulerConfig,
        configured: PolicyProfile,
        adaptive: bool,
    ) -> Arc<Self> {
        let session = Self::build(config, configured, adaptive);
        if adaptive {
            spawn_adaptive_driver(Arc::downgrade(&session));
        }
        session
    }

    /// The session itself, with no driver task: `new` spawns the 1 Hz loop on top of this. Tests
    /// that force a tick sequence need the same state without a second, unsolicited sampler.
    fn build(config: SchedulerConfig, configured: PolicyProfile, adaptive: bool) -> Arc<Self> {
        Arc::new(Self {
            to_peer: Arbiter::spawn(config),
            from_peer: Arbiter::spawn(config),
            chunk: [
                AtomicUsize::new(config.quantum_bytes),
                AtomicUsize::new(config.quantum_bytes),
            ],
            next_flow: AtomicU64::new(1),
            adaptation: adaptive.then(|| Mutex::new(Adaptation::new(configured))),
        })
    }

    /// An adaptive session the test drives itself: the real arbiters, the real controller and the
    /// real apply path, but no 1 Hz task ticking behind the forced sequence.
    #[cfg(test)]
    fn without_driver(config: SchedulerConfig, configured: PolicyProfile) -> Arc<Self> {
        Self::build(config, configured, true)
    }

    /// Bytes one read may carry for the direction, and therefore the most a flow queues.
    fn chunk(&self, direction: Direction) -> &AtomicUsize {
        match direction {
            Direction::ToPeer => &self.chunk[0],
            Direction::FromPeer => &self.chunk[1],
        }
    }

    /// Every change adaptation made, for the evidence record.
    #[allow(dead_code)] // read by the measurement harness, which also builds outside `cfg(test)`
    pub(crate) fn changes(&self) -> Vec<DirectionChange> {
        self.adaptation
            .as_ref()
            .and_then(|adaptation| adaptation.lock().ok())
            .map(|adaptation| adaptation.transcript().to_vec())
            .unwrap_or_default()
    }

    /// Every queueing-delay estimate adaptation computed, for the evidence record.
    #[allow(dead_code)] // read by the measurement harness, which also builds outside `cfg(test)`
    pub(crate) fn estimates(&self) -> Vec<(Direction, Duration)> {
        self.adaptation
            .as_ref()
            .and_then(|adaptation| adaptation.lock().ok())
            .map(|adaptation| adaptation.history().to_vec())
            .unwrap_or_default()
    }

    /// One accepted change: reconfigure that direction's arbiter and keep its read bound in step.
    async fn apply(&self, change: DirectionChange) {
        let profile = change.change.to;
        let config = SchedulerConfig {
            quantum_bytes: profile.quantum_bytes(),
            max_queued_bytes: profile.max_queued_bytes(),
        };
        let arbiter = match change.direction {
            Direction::ToPeer => &self.to_peer,
            Direction::FromPeer => &self.from_peer,
        };
        if arbiter.configure(config).await {
            self.chunk(change.direction)
                .store(config.quantum_bytes, Ordering::Relaxed);
            // The running row moved: publish it, or the agent would keep reporting the old one
            // for the rest of the process's life (ADR-0047 'profile.running').
            crate::policy_status::note_row(profile);
            eprintln!("{change}");
        }
    }

    /// One slow-loop tick: read this session's own counters, then decide and apply. Split so the
    /// driver has no decision logic.
    async fn tick(&self, started: Instant) {
        let to_peer = self.to_peer.stats().await.map(counters);
        let from_peer = self.from_peer.stats().await.map(counters);
        self.tick_with(started.elapsed(), to_peer, from_peer).await;
    }

    /// The decide-and-apply half of a tick, given each direction's sample or `None` for a missed
    /// one. The driver only ever passes [`Self::tick`]'s live reads; the rollback/fail-safe matrix
    /// (TP-4.2) forces them, so every matrix row runs through the same controller, the same
    /// `Command::Configure` and the same read-bound update as live traffic.
    async fn tick_with(
        &self,
        now: Duration,
        to_peer: Option<Counters>,
        from_peer: Option<Counters>,
    ) {
        let Some(adaptation) = &self.adaptation else {
            return;
        };
        let changes = adaptation
            .lock()
            .map(|mut adaptation| adaptation.tick(now, to_peer, from_peer))
            .unwrap_or_default();
        for change in changes {
            self.apply(change).await;
        }
    }

    /// Registers one stream in both directions under one id.
    pub(crate) fn open(&self) -> (FlowHandle, FlowHandle) {
        let id = FlowId(self.next_flow.fetch_add(1, Ordering::Relaxed));
        (self.to_peer.flow(id), self.from_peer.flow(id))
    }

    /// Live flow count of both directions, for tests and diagnostics.
    #[cfg(test)]
    pub(crate) async fn flows(&self) -> (usize, usize) {
        let to_peer = self.to_peer.stats().await.map_or(0, |stats| stats.flows);
        let from_peer = self.from_peer.stats().await.map_or(0, |stats| stats.flows);
        (to_peer, from_peer)
    }
}

/// The pack's counters as the controller's sample. `violations` is the arbiter's own guard
/// counter: it rises only above the configured row's cap, so the 1 Hz loop sees a live guard
/// signal (ADR-0056 decision 8), not a fabricated zero or a forced sample.
fn counters(stats: ArbiterStats) -> Counters {
    Counters {
        queued_bytes: stats.queued_bytes,
        granted_bytes: stats.granted_bytes,
        overflow_refusals: stats.overflow_refusals,
        violations: stats.guard_violations,
    }
}

/// The 1 Hz slow loop, one per adaptive session. It holds the session weakly, so it stops as soon
/// as the session and its arbiters go away and cannot keep a finished session alive.
fn spawn_adaptive_driver(session: Weak<PolicySession>) {
    tokio::spawn(async move {
        let started = Instant::now();
        loop {
            tokio::time::sleep(SAMPLE_PERIOD).await;
            let Some(session) = session.upgrade() else {
                return;
            };
            session.tick(started).await;
        }
    });
}

/// Relay both directions under the pack's scheduler until each side closes, failing a direction
/// that is idle too long. Semantics match `tunnel::relay_with_idle_timeout`: the same idle timeout
/// per direction, the same shutdown on EOF, and the same per-direction byte counters. The only
/// difference is that a write waits for this flow's turn in the direction's rotation.
pub(crate) async fn relay_with_policy<A, B>(
    peer: A,
    local: B,
    idle: Duration,
    session: &PolicySession,
) -> io::Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    crate::session_status::stream_opened();
    let (to_peer, from_peer) = session.open();
    let (a_read, a_write) = tokio::io::split(peer);
    let (b_read, b_write) = tokio::io::split(local);
    let result = tokio::try_join!(
        pump_scheduled(
            a_read,
            b_write,
            idle,
            &from_peer,
            session.chunk(Direction::FromPeer),
            crate::session_status::add_from_peer,
        ),
        pump_scheduled(
            b_read,
            a_write,
            idle,
            &to_peer,
            session.chunk(Direction::ToPeer),
            crate::session_status::add_to_peer,
        )
    );
    from_peer.close().await;
    to_peer.close().await;
    result?;
    Ok(())
}

/// One direction: read up to one chunk, wait for this flow's turn, write exactly that chunk.
/// A read of zero shuts the writer down and ends the direction, as the unscheduled pump does.
async fn pump_scheduled<R, W>(
    mut reader: R,
    mut writer: W,
    idle: Duration,
    flow: &FlowHandle,
    chunk: &AtomicUsize,
    count: fn(usize),
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = Vec::new();
    loop {
        // An adaptation step changes this direction's quantum; the next read must not exceed it,
        // or the request could be larger than the new row's queue cap.
        let size = chunk.load(Ordering::Relaxed).max(1);
        buffer.resize(size, 0);
        let read = timeout(idle, reader.read(&mut buffer))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "stream idle timeout"))??;
        if read == 0 {
            writer.shutdown().await?;
            return Ok(());
        }
        let mut written = 0;
        while written < read {
            // A change can land while this read is in flight, so the offer is re-capped to the
            // quantum in force now: the arbiter refuses an offer past its row's cap.
            let want = (read - written).min(chunk.load(Ordering::Relaxed).max(1));
            let grant = flow.acquire(want).await?;
            if grant > want {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "scheduler granted more than the flow offered",
                ));
            }
            writer.write_all(&buffer[written..written + grant]).await?;
            count(grant);
            written += grant;
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio_yamux::config::INITIAL_STREAM_WINDOW;

    use super::*;

    #[test]
    fn switch_is_off_unless_explicitly_enabled() {
        assert_eq!(PolicyRelay::parse(""), None);
        assert_eq!(PolicyRelay::parse("  "), None);
        assert_eq!(PolicyRelay::parse("off"), None);
        assert_eq!(PolicyRelay::parse("OFF"), None);
        assert_eq!(
            PolicyRelay::from_env_and_profile(None),
            None,
            "the switch is unset in tests"
        );
    }

    #[test]
    fn switch_parses_a_window_and_refuses_anything_else() {
        assert_eq!(
            PolicyRelay::parse("window-cap:1048576"),
            Some(PolicyRelay {
                stream_window: 1024 * 1024,
                scheduler: None,
            })
        );
        assert_eq!(
            PolicyRelay::parse("window-cap"),
            Some(PolicyRelay {
                stream_window: DEFAULT_STREAM_WINDOW,
                scheduler: None,
            })
        );
        assert_eq!(
            PolicyRelay::parse("Window-Cap: 262144"),
            Some(PolicyRelay {
                stream_window: 256 * 1024,
                scheduler: None,
            })
        );
        // Below Yamux's initial window both ends would disagree about the window at stream open.
        assert_eq!(PolicyRelay::parse("window-cap:65536"), None);
        assert_eq!(
            PolicyRelay::parse(&format!("window-cap:{}", INITIAL_STREAM_WINDOW * 2))
                .map(|p| p.stream_window),
            Some(INITIAL_STREAM_WINDOW * 2)
        );
        assert_eq!(PolicyRelay::parse("window-cap:lots"), None);
        assert_eq!(PolicyRelay::parse("window-cap:0"), None);
        // M1's relay-level credit budget was rejected by the experiments; it is not a switch.
        assert_eq!(PolicyRelay::parse("credit-budget:65536"), None);
        assert_eq!(PolicyRelay::parse("on"), None);
    }

    #[test]
    fn sched_switch_selects_the_scheduler_with_bounded_queues() {
        let default = PolicyRelay::parse("sched").expect("sched");
        assert_eq!(default.stream_window, DEFAULT_STREAM_WINDOW);
        assert_eq!(
            default.scheduler,
            Some(SchedulerConfig {
                quantum_bytes: 16 * 1024,
                max_queued_bytes: 64 * 1024,
            })
        );
        let explicit = PolicyRelay::parse("sched:8192:32768").expect("sched with bounds");
        assert_eq!(
            explicit.scheduler,
            Some(SchedulerConfig {
                quantum_bytes: 8 * 1024,
                max_queued_bytes: 32 * 1024,
            })
        );
        // A quantum above the window would hold more than the in-flight bound; the cap must hold at
        // least one quantum or the scheduler would refuse every request.
        assert_eq!(PolicyRelay::parse("sched:262144:65536"), None);
        assert_eq!(PolicyRelay::parse("sched:32768:16384"), None);
        assert_eq!(PolicyRelay::parse("sched:512:2048"), None);
        assert_eq!(PolicyRelay::parse("sched:16384:2097152"), None);
        assert_eq!(PolicyRelay::parse("sched:16384:nope"), None);
        assert_eq!(PolicyRelay::parse("sched:16384:65536:extra"), None);
        assert_eq!(PolicyRelay::parse("sched:-1"), None);
        // The scheduler never turns the window cap off on its own.
        assert_eq!(
            PolicyRelay::parse("sched").map(|p| p.stream_window),
            Some(DEFAULT_STREAM_WINDOW)
        );
    }

    #[tokio::test]
    async fn session_relays_both_directions_and_releases_its_flows() {
        let session = PolicySession::with_config(SchedulerConfig {
            quantum_bytes: 4 * 1024,
            max_queued_bytes: 16 * 1024,
        });
        let (app, local) = tokio::io::duplex(8 * 1024);
        let (peer, tunnel) = tokio::io::duplex(8 * 1024);
        let relay = tokio::spawn({
            let session = Arc::clone(&session);
            async move { relay_with_policy(tunnel, local, Duration::from_secs(5), &session).await }
        });
        // Echo on the peer side so both directions carry bytes.
        let peer = tokio::spawn(async move {
            let (mut read, mut write) = tokio::io::split(peer);
            let mut buffer = [0_u8; 1024];
            while let Ok(read) = read.read(&mut buffer).await {
                if read == 0 || write.write_all(&buffer[..read]).await.is_err() {
                    break;
                }
            }
        });
        let (mut receive, mut send) = tokio::io::split(app);
        for round in 0..8_u8 {
            let payload = [round; 3_000];
            send.write_all(&payload).await.expect("write app");
            let mut echo = vec![0_u8; payload.len()];
            receive.read_exact(&mut echo).await.expect("echo back");
            assert_eq!(echo, payload);
        }
        // Closing the app side mid-stream must end the relay instead of wedging it. `split` shares
        // one stream between its halves, so dropping the write half would not close it.
        send.shutdown().await.expect("close app side");
        drop(send);
        let result = tokio::time::timeout(Duration::from_secs(5), relay)
            .await
            .expect("relay must finish after the app side closes")
            .expect("relay task");
        peer.abort();
        assert!(result.is_ok(), "relay ended with {result:?}");
        let mut flows = session.flows().await;
        for _ in 0..100 {
            if flows == (0, 0) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
            flows = session.flows().await;
        }
        assert_eq!(flows, (0, 0), "no flow may outlive its relay");
    }

    /// ADR-0056 decision 10: with the pack switch unset, `policy.adaptive` constructs nothing, so
    /// the data path is exactly today's. The switch, not the tunnel config, decides whether the
    /// module runs at all (ADR-0048).
    #[test]
    fn adaptation_cannot_start_without_the_pack_switch() {
        use crate::policy_profile::PolicyProfile;
        assert!(
            PolicySession::from_env_and_profile(Some(PolicyProfile::Balanced), true).is_none(),
            "the switch is unset in tests"
        );
        assert!(PolicySession::from_env_and_profile(None, true).is_none());
        assert!(PolicySession::from_env_and_profile(None, false).is_none());
    }

    /// Without the opt-in the session has no controller at all: no sampler task, no changes, and
    /// both directions keep the configured quantum for the session's whole life.
    #[tokio::test]
    async fn an_unconfigured_session_has_no_adaptation() {
        let session = PolicySession::with_config(SchedulerConfig {
            quantum_bytes: 4 * 1024,
            max_queued_bytes: 16 * 1024,
        });
        assert!(session.adaptation.is_none());
        assert_eq!(
            session.chunk(Direction::ToPeer).load(Ordering::Relaxed),
            4 * 1024
        );
        assert_eq!(
            session.chunk(Direction::FromPeer).load(Ordering::Relaxed),
            4 * 1024
        );
        assert!(session.changes().is_empty());
        assert!(session.estimates().is_empty());
    }

    /// ADR-0056 decision 2: an accepted change is applied to that direction's scheduler pair and
    /// its read bound together, and to nothing else. The other direction keeps its row.
    #[tokio::test]
    async fn an_accepted_change_reaches_only_its_direction() {
        use crate::policy_adaptive::{Cause, Change};
        use crate::policy_profile::PolicyProfile;
        let session = PolicySession::new(
            SchedulerConfig {
                quantum_bytes: PolicyProfile::Gaming.quantum_bytes(),
                max_queued_bytes: PolicyProfile::Gaming.max_queued_bytes(),
            },
            PolicyProfile::Gaming,
            true,
        );
        let (to_peer, _) = session.open();
        session
            .apply(DirectionChange {
                direction: Direction::ToPeer,
                change: Change {
                    at: Duration::from_secs(4),
                    from: PolicyProfile::Gaming,
                    to: PolicyProfile::Trading,
                    cause: Cause::QueueDelayHigh,
                    queue_delay: Duration::from_millis(80),
                },
            })
            .await;
        assert_eq!(
            session.chunk(Direction::ToPeer).load(Ordering::Relaxed),
            PolicyProfile::Trading.quantum_bytes()
        );
        assert_eq!(
            session.chunk(Direction::FromPeer).load(Ordering::Relaxed),
            PolicyProfile::Gaming.quantum_bytes(),
            "the other direction was not the one that queued"
        );
        // The running arbiter enforces the new quantum on the very next grant, no restart.
        assert_eq!(
            to_peer.acquire(4096).await.expect("grant"),
            PolicyProfile::Trading.quantum_bytes()
        );
        let stats = session.to_peer.stats().await.expect("stats");
        assert_eq!(
            stats.granted_bytes,
            PolicyProfile::Trading.quantum_bytes() as u64
        );
    }

    /// TP-4.2 harness: the rollback/fail-safe matrix needs change events, and a loopback run
    /// produces none (the arbiter queue is a rendezvous, so its 1 Hz sample reads zero). These
    /// drive the real session — controller, `Command::Configure` and read-bound update — with
    /// scripted per-second counters instead of measured ones. Every second and every millisecond
    /// below is forced, not observed; the live path is unchanged.
    fn config_for(profile: PolicyProfile) -> SchedulerConfig {
        SchedulerConfig {
            quantum_bytes: profile.quantum_bytes(),
            max_queued_bytes: profile.max_queued_bytes(),
        }
    }

    /// One forced sample: with the arbiter adding exactly 1 MB of grants per second,
    /// `delay_ms * 1_000` queued bytes is `delay_ms` milliseconds of queueing delay.
    fn forced(second: u64, delay_ms: usize) -> Option<Counters> {
        forced_with(second, delay_ms, 0, 0)
    }

    fn forced_with(
        second: u64,
        delay_ms: usize,
        overflow_refusals: u64,
        violations: u64,
    ) -> Option<Counters> {
        Some(Counters {
            queued_bytes: delay_ms * 1_000,
            granted_bytes: second * 1_000_000,
            overflow_refusals,
            violations,
        })
    }

    async fn tick_at(
        session: &PolicySession,
        second: u64,
        to_peer: Option<Counters>,
        from_peer: Option<Counters>,
    ) {
        session
            .tick_with(Duration::from_secs(second), to_peer, from_peer)
            .await;
    }

    /// What the direction's live arbiter grants for one offer, i.e. the row in force right now.
    async fn granted(session: &PolicySession, offer: usize) -> usize {
        let (flow, _) = session.open();
        let grant = flow
            .acquire(offer)
            .await
            .expect("the arbiter grants under the row in force");
        flow.close().await;
        grant
    }

    /// ADR-0056 decision 7: a down-step that fails its 60 s verification is reverted to the row that
    /// was already verified — in the running arbiter and in the read bound, not just in the
    /// controller's return value.
    #[tokio::test]
    async fn a_failed_verification_rolls_the_live_arbiter_and_read_bound_back() {
        use crate::policy_adaptive::Cause;

        let configured = PolicyProfile::Balanced;
        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=3 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        assert_eq!(session.changes().len(), 1);
        assert_eq!(session.changes()[0].change.to, PolicyProfile::Gaming);
        assert_eq!(
            session.chunk(Direction::ToPeer).load(Ordering::Relaxed),
            PolicyProfile::Gaming.quantum_bytes()
        );
        assert_eq!(
            granted(&session, configured.quantum_bytes()).await,
            PolicyProfile::Gaming.quantum_bytes()
        );

        // The window closes with a 5% improvement: below the 10% floor, so the row is bad.
        tick_at(&session, 63, forced(63, 95), forced(63, 5)).await;
        let changes = session.changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[1].change.cause, Cause::NoImprovement);
        assert_eq!(changes[1].change.from, PolicyProfile::Gaming);
        assert_eq!(changes[1].change.to, PolicyProfile::Balanced);
        assert_eq!(
            session.chunk(Direction::ToPeer).load(Ordering::Relaxed),
            PolicyProfile::Balanced.quantum_bytes()
        );
        assert_eq!(
            granted(&session, configured.quantum_bytes()).await,
            PolicyProfile::Balanced.quantum_bytes()
        );
    }

    /// ADR-0056 decision 8: a guard violation restores the configured row on the same tick and
    /// suspends adaptation for its whole 300 s window; the loop then resumes on fresh persistence.
    #[tokio::test]
    async fn a_guard_violation_restores_the_live_row_and_suspends_the_loop() {
        use crate::policy_adaptive::Cause;

        let configured = PolicyProfile::Balanced;
        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=3 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        assert_eq!(session.changes()[0].change.to, PolicyProfile::Gaming);

        tick_at(&session, 4, forced_with(4, 100, 0, 1), forced(4, 5)).await;
        let changes = session.changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[1].change.cause, Cause::GuardViolation);
        assert_eq!(changes[1].change.to, PolicyProfile::Balanced);
        assert_eq!(
            session.chunk(Direction::ToPeer).load(Ordering::Relaxed),
            PolicyProfile::Balanced.quantum_bytes()
        );
        assert_eq!(
            granted(&session, configured.quantum_bytes()).await,
            PolicyProfile::Balanced.quantum_bytes()
        );

        // Sustained congestion for the whole suspension changes nothing.
        for second in 5..304 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        assert_eq!(
            session.changes().len(),
            2,
            "no change inside the suspension"
        );

        // The window ends at 4 + 300; three fresh samples then step down again (the dwell since
        // the restore and the three-change budget both allow it).
        for second in 304..=306 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        let changes = session.changes();
        assert_eq!(changes.len(), 3, "adaptation resumes after the suspension");
        assert_eq!(changes[2].change.to, PolicyProfile::Gaming);
        assert_eq!(changes[2].change.at, Duration::from_secs(306));
    }

    /// ADR-0056 decisions 7-8: a cooldown earned by a failed row is not shortened by a later guard
    /// suspension — the row stays unreachable until its own 600 s window closes.
    #[tokio::test]
    async fn a_failed_row_stays_cooled_down_across_a_suspend_and_resume_cycle() {
        let configured = PolicyProfile::Bulk;
        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=3 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        assert_eq!(session.changes()[0].change.to, PolicyProfile::Streaming);

        // Fails verification at 63 s: back to Bulk, and Streaming is cooled down until 663.
        tick_at(&session, 63, forced(63, 95), forced(63, 5)).await;
        assert_eq!(session.changes().len(), 2);

        // A violation arrives while the configured row is already in force: nothing changes, but
        // the loop is suspended until 364.
        tick_at(&session, 64, forced_with(64, 100, 0, 1), forced(64, 5)).await;
        assert_eq!(session.changes().len(), 2, "already at the configured row");

        for second in 65..663 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        assert_eq!(
            session.changes().len(),
            2,
            "the cooled-down row stays unreachable past the suspension"
        );

        // At exactly 663 s the cooldown closes and the persistence already built is enough.
        tick_at(&session, 663, forced(663, 100), forced(663, 5)).await;
        let changes = session.changes();
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[2].change.to, PolicyProfile::Streaming);
        assert_eq!(changes[2].change.at, Duration::from_secs(663));
    }

    /// ADR-0056 decision 8: three missed samples restore the configured row, and a gap clears the
    /// persistence streak rather than carrying it across.
    #[tokio::test]
    async fn a_stale_gap_resets_the_streak_and_restores_the_live_row() {
        use crate::policy_adaptive::Cause;

        let configured = PolicyProfile::Streaming;
        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=3 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        assert_eq!(session.changes()[0].change.to, PolicyProfile::Balanced);

        // Two samples, one missed tick, then two more: four congested samples and still no change.
        for second in [4_u64, 5] {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        tick_at(&session, 6, None, None).await;
        for second in [7_u64, 8] {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        assert_eq!(
            session.changes().len(),
            1,
            "the gap reset the persistence streak"
        );

        for second in 9..=11 {
            tick_at(&session, second, None, None).await;
        }
        let changes = session.changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[1].change.cause, Cause::StaleSignals);
        assert_eq!(changes[1].change.to, PolicyProfile::Streaming);
        assert_eq!(
            session.chunk(Direction::ToPeer).load(Ordering::Relaxed),
            PolicyProfile::Streaming.quantum_bytes()
        );
        assert_eq!(
            granted(&session, configured.quantum_bytes()).await,
            PolicyProfile::Streaming.quantum_bytes()
        );
    }

    /// ADR-0056 decision 8's other trigger: an `overflow_refusals` increase takes the same
    /// immediate restore-and-suspend path as a guard violation.
    #[tokio::test]
    async fn an_overflow_restores_the_live_row_and_suspends_the_loop() {
        use crate::policy_adaptive::Cause;

        let configured = PolicyProfile::Balanced;
        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=3 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        assert_eq!(session.changes()[0].change.to, PolicyProfile::Gaming);

        tick_at(&session, 4, forced_with(4, 100, 1, 0), forced(4, 5)).await;
        let changes = session.changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[1].change.cause, Cause::Overflow);
        assert_eq!(changes[1].change.to, PolicyProfile::Balanced);
        assert_eq!(
            session.chunk(Direction::ToPeer).load(Ordering::Relaxed),
            PolicyProfile::Balanced.quantum_bytes()
        );
        for second in 5..304 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        assert_eq!(session.changes().len(), 2, "the suspension holds");
    }

    /// The sample must carry the arbiter's real guard counter. If anyone hardcodes the field back
    /// to zero, this fails even though the forced matrix would still pass.
    #[test]
    fn the_live_sample_carries_the_arbiters_guard_counter() {
        let stats = ArbiterStats {
            guard_violations: 3,
            ..ArbiterStats::default()
        };
        assert_eq!(counters(stats).violations, 3);
    }

    /// ADR-0056 decision 8 on a real signal: no forced `Counters` feeds the guard. The live
    /// arbiter refuses a request above the row's cap, the guard counter rises, and the next tick's
    /// own counter read restores the configured row and suspends adaptation.
    #[tokio::test]
    async fn a_live_arbiter_guard_violation_restores_the_row_without_forced_samples() {
        use crate::policy_adaptive::Cause;

        let configured = PolicyProfile::Balanced;
        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=3 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        assert_eq!(session.changes()[0].change.to, PolicyProfile::Gaming);
        assert_eq!(
            granted(&session, configured.quantum_bytes()).await,
            PolicyProfile::Gaming.quantum_bytes()
        );

        // The real signal: a request above the row's cap is refused by the live arbiter.
        let (flow, _) = session.open();
        let over_cap = config_for(configured).max_queued_bytes + 1;
        assert!(
            flow.acquire(over_cap).await.is_err(),
            "the arbiter refuses bytes above the cap"
        );
        flow.close().await;
        let stats = session.to_peer.stats().await.expect("stats");
        assert_eq!(
            stats.guard_violations, 1,
            "the signal came from the arbiter"
        );
        assert_eq!(counters(stats).violations, 1);

        // The controller's own live read, not a scripted counter set.
        let to_peer = session.to_peer.stats().await.map(counters);
        let from_peer = session.from_peer.stats().await.map(counters);
        session
            .tick_with(Duration::from_secs(4), to_peer, from_peer)
            .await;

        let changes = session.changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[1].change.cause, Cause::GuardViolation);
        assert_eq!(changes[1].change.to, configured);
        assert_eq!(
            session.chunk(Direction::ToPeer).load(Ordering::Relaxed),
            configured.quantum_bytes()
        );
        assert_eq!(
            granted(&session, configured.quantum_bytes()).await,
            configured.quantum_bytes(),
            "the running row is back at the safe one"
        );
    }

    /// #239's condition matrix at the decision boundary: each condition is the 1 Hz sample sequence
    /// it would produce, forced into the real session. The network itself is not simulated (no
    /// loss, jitter or path change is induced), so these check the controller's response to those
    /// sequences: clean and jittery change nothing, sustained congestion steps once, a permanent
    /// path improvement keeps the step and returns to the configured row.
    #[tokio::test]
    async fn forced_condition_matrix_covers_clean_congested_lossy_jittery_and_path_change() {
        let configured = PolicyProfile::Balanced;

        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=60 {
            tick_at(&session, second, forced(second, 5), forced(second, 5)).await;
        }
        assert!(
            session.changes().is_empty(),
            "clean traffic changes nothing"
        );

        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=3 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        let changes = session.changes();
        assert_eq!(changes.len(), 1, "congested traffic steps once");
        assert_eq!(changes[0].change.to, PolicyProfile::Gaming);

        // Lossy: one 80 ms burst sample at a time, never three in a row.
        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=120 {
            let delay = if second % 3 == 0 { 80 } else { 5 };
            tick_at(&session, second, forced(second, delay), forced(second, 5)).await;
        }
        assert!(
            session.changes().is_empty(),
            "isolated loss bursts do not accumulate a streak"
        );

        // Jittery: alternating either side of the 50 ms threshold.
        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=120 {
            let delay = if second % 2 == 0 { 55 } else { 45 };
            tick_at(&session, second, forced(second, delay), forced(second, 5)).await;
        }
        assert!(
            session.changes().is_empty(),
            "jitter around the threshold never persists"
        );

        // Path change: sustained congestion, then a permanent drop to 15 ms. The step is kept and
        // the calm samples walk it back to the configured row.
        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=3 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        for second in 4..=62 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        // The improved sample that closes the window already counts as the first calm one.
        tick_at(&session, 63, forced(63, 15), forced(63, 5)).await;
        for second in 64..=65 {
            tick_at(&session, second, forced(second, 15), forced(second, 5)).await;
        }
        let changes = session.changes();
        assert_eq!(changes.len(), 2, "the improved step is kept, then returned");
        assert_eq!(changes[0].change.to, PolicyProfile::Gaming);
        assert_eq!(changes[1].change.to, configured);
        assert_eq!(changes[1].change.at, Duration::from_secs(65));
    }

    /// #239's flapping check and long run: ten minutes of alternating 4 s congestion and 8 s calm,
    /// the plan's ceiling, forced into one session. Every change is one rung, never above the
    /// configured row, and the loop stops moving instead of oscillating once the candidate rows are
    /// cooled down.
    #[tokio::test]
    async fn forced_flapping_run_is_bounded_and_settles() {
        let configured = PolicyProfile::Balanced;
        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=600 {
            let delay = if second % 12 < 4 { 100 } else { 5 };
            tick_at(&session, second, forced(second, delay), forced(second, 5)).await;
        }
        let changes = session.changes();
        let rung = |profile: PolicyProfile| {
            crate::policy_adaptive::LADDER
                .iter()
                .position(|row| *row == profile)
                .expect("ratified row")
        };
        for change in &changes {
            assert_eq!(
                rung(change.change.to).abs_diff(rung(change.change.from)),
                1,
                "every change is one rung: {change:?}"
            );
            assert!(
                rung(change.change.to) <= rung(configured),
                "adaptation never rises above the configured row"
            );
        }
        assert!(
            changes.len() <= 4,
            "ten flapping minutes produce at most four changes, got {}",
            changes.len()
        );
        let last = changes.last().expect("the storm does move once").change.at;
        assert!(
            last <= Duration::from_secs(200),
            "the loop settles instead of oscillating, last change at {last:?}"
        );
    }

    /// ADR-0056 decisions 9-10: without `policy.adaptive` there is no controller, so the same
    /// forced guard and overflow samples change nothing at all.
    #[tokio::test]
    async fn forced_fail_safe_samples_change_nothing_without_the_opt_in() {
        let configured = PolicyProfile::Balanced;
        let session = PolicySession::with_config(config_for(configured));
        for second in 0..5 {
            tick_at(
                &session,
                second,
                forced_with(second, 400, 9, 9),
                forced(second, 5),
            )
            .await;
        }
        assert!(session.changes().is_empty());
        assert!(session.estimates().is_empty());
        assert_eq!(
            session.chunk(Direction::ToPeer).load(Ordering::Relaxed),
            configured.quantum_bytes()
        );
        assert_eq!(
            granted(&session, configured.quantum_bytes()).await,
            configured.quantum_bytes()
        );
    }

    /// The timeline the TP-4.2 evidence record quotes: a scripted sequence, not a network
    /// measurement. Change 1 at the third congested second, change 2 three calm seconds after the
    /// step above it was kept.
    #[tokio::test]
    async fn the_forced_matrix_timeline_is_reproducible() {
        let configured = PolicyProfile::Bulk;
        let session = PolicySession::without_driver(config_for(configured), configured);
        tick_at(&session, 0, forced(0, 5), forced(0, 5)).await;
        for second in 1..=3 {
            tick_at(&session, second, forced(second, 100), forced(second, 5)).await;
        }
        assert_eq!(session.changes()[0].change.to, PolicyProfile::Streaming);
        // A 40% improvement keeps the step when the window closes, then calm samples step it up.
        tick_at(&session, 63, forced(63, 60), forced(63, 5)).await;
        for second in 64..=66 {
            tick_at(&session, second, forced(second, 5), forced(second, 5)).await;
        }
        let changes = session.changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[1].change.to, PolicyProfile::Bulk);
        assert_eq!(changes[1].change.at, Duration::from_secs(66));
        let at: Vec<u64> = changes
            .iter()
            .map(|change| change.change.at.as_millis() as u64)
            .collect();
        assert_eq!(at, vec![3_000, 66_000]);
        println!(
            "POLICY_TP42_MATRIX {{\"label\":\"forced\",\"changes\":{},\"at_ms\":{at:?},\"gaps_ms\":[63000]}}",
            changes.len()
        );
    }

    #[test]
    fn a_configured_profile_supplies_the_ratified_numbers_and_the_switch_only_enables_them() {
        use crate::policy_profile::PolicyProfile;
        for (profile, window, quantum, cap) in [
            (PolicyProfile::Balanced, 256 * 1024, 16 * 1024, 64 * 1024),
            (PolicyProfile::Gaming, 256 * 1024, 4 * 1024, 16 * 1024),
            (PolicyProfile::Trading, 256 * 1024, 2 * 1024, 8 * 1024),
            (PolicyProfile::Streaming, 512 * 1024, 32 * 1024, 128 * 1024),
            (PolicyProfile::Bulk, 2 * 1024 * 1024, 64 * 1024, 256 * 1024),
        ] {
            assert_eq!(
                PolicyRelay::resolve("sched", Some(profile)),
                Some(PolicyRelay {
                    stream_window: window,
                    scheduler: Some(SchedulerConfig {
                        quantum_bytes: quantum,
                        max_queued_bytes: cap,
                    }),
                }),
                "{profile} must resolve to the ADR-0031 row"
            );
            // The switch decides whether the pack runs; its arguments do not override a profile.
            assert_eq!(
                PolicyRelay::resolve("sched:4096:8192", Some(profile)),
                PolicyRelay::resolve("sched", Some(profile)),
            );
            assert_eq!(
                PolicyRelay::resolve("window-cap:1048576", Some(profile))
                    .map(|relay| relay.stream_window),
                Some(window)
            );
            // Switch off wins, whatever the profile: the data path stays today's.
            assert_eq!(PolicyRelay::resolve("off", Some(profile)), None);
            assert_eq!(PolicyRelay::resolve("", Some(profile)), None);
        }
    }

    #[test]
    fn no_profile_keeps_the_switch_numbers_byte_for_byte() {
        for switch in ["sched", "sched:8192:65536", "window-cap:1048576", "off", ""] {
            assert_eq!(
                PolicyRelay::resolve(switch, None),
                PolicyRelay::parse(switch),
                "the absent-profile path must be exactly the switch's, as before TP-3.1"
            );
        }
    }

    #[test]
    fn profile_numbers_stay_inside_the_relay_bounds() {
        use crate::policy_profile::PolicyProfile;
        for profile in [
            PolicyProfile::Balanced,
            PolicyProfile::Gaming,
            PolicyProfile::Trading,
            PolicyProfile::Streaming,
            PolicyProfile::Bulk,
        ] {
            assert!(profile.window_bytes() >= tokio_yamux::config::INITIAL_STREAM_WINDOW);
            assert!((1024..=DEFAULT_STREAM_WINDOW as usize).contains(&profile.quantum_bytes()));
            assert!(profile.quantum_bytes() <= profile.max_queued_bytes());
            assert!(profile.max_queued_bytes() <= MAX_QUEUE_CAP_BYTES);
            let scheduler = PolicyRelay::resolve("sched", Some(profile))
                .and_then(|relay| relay.scheduler)
                .expect("a profile always carries a scheduler");
            assert_eq!(scheduler.quantum_bytes, profile.quantum_bytes());
        }
    }
}
