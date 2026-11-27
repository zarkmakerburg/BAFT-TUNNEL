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
        let session = Arc::new(Self {
            to_peer: Arbiter::spawn(config),
            from_peer: Arbiter::spawn(config),
            chunk: [
                AtomicUsize::new(config.quantum_bytes),
                AtomicUsize::new(config.quantum_bytes),
            ],
            next_flow: AtomicU64::new(1),
            adaptation: adaptive.then(|| Mutex::new(Adaptation::new(configured))),
        });
        if adaptive {
            spawn_adaptive_driver(Arc::downgrade(&session));
        }
        session
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
            eprintln!("{change}");
        }
    }

    /// One slow-loop tick, split out so the driver has no decision logic and tests can drive it.
    async fn tick(&self, started: Instant) {
        let Some(adaptation) = &self.adaptation else {
            return;
        };
        let to_peer = self.to_peer.stats().await.map(counters);
        let from_peer = self.from_peer.stats().await.map(counters);
        let changes = adaptation
            .lock()
            .map(|mut adaptation| adaptation.tick(started.elapsed(), to_peer, from_peer))
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

/// The pack's counters as the controller's sample. The scheduler path has no
/// `Guarded<TrafficPolicy>` in it (`policy.rs`'s wrapper is not instantiated by the relay), so there
/// is no live interface-violation counter to report: the field is zero and the guard fail-safe is
/// covered by the controller's own tests, not by live traffic.
fn counters(stats: ArbiterStats) -> Counters {
    Counters {
        queued_bytes: stats.queued_bytes,
        granted_bytes: stats.granted_bytes,
        overflow_refusals: stats.overflow_refusals,
        violations: 0,
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
