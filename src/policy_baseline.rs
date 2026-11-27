//! Policy Pack measurement harness, TP-1.2 first stage (ADR-0031, docs/POLICY_PACK_TP-0.1_RESEARCH.md).
//!
//! Test-only. Measures the round-trip time of a small interactive stream with and without a
//! competing bulk stream, through the product's own `yamux_config` and `relay_with_idle_timeout`
//! (the real relay path, both ends) over loopback TCP joined by a rate-limited proxy. Evidence
//! level of every number is `simulated` (never VM or LAB). It measures today's data path
//! (Baseline, pack off) plus the ADR-0048 mechanism variants M1/M2/M3 behind test-only switches and
//! ADR-0056's adaptive arm, which starts from a profile row and may select another one.
//!
//! Run: `cargo test --release policy_baseline -- --ignored --nocapture`; the adaptive arm is
//! `adaptive_controller_experiments`.
//! The comparison run reads `POLICY_RATE`, `POLICY_REPEATS` and `POLICY_SECONDS`.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use futures::StreamExt;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt, DuplexStream, duplex},
    net::{TcpListener, TcpStream},
};
use tokio_yamux::Session;

#[cfg(feature = "policy-pack")]
use crate::policy::Direction;
use crate::tunnel::{relay_with_idle_timeout, yamux_config};

/// One end's scheduler state for the `sched` arm. Without the feature the arm does not exist and the
/// alias is only there to keep the harness's helper signatures uniform.
#[cfg(feature = "policy-pack")]
type ArmPolicy = std::sync::Arc<crate::policy_relay::PolicySession>;
#[cfg(not(feature = "policy-pack"))]
#[derive(Debug)]
struct PackDisabled;
#[cfg(not(feature = "policy-pack"))]
type ArmPolicy = std::sync::Arc<PackDisabled>;

const PING: usize = 64;
const PUMP_READ: usize = 16 * 1024;
const IDLE: Duration = Duration::from_secs(900);
/// Product Yamux per-stream window (`tunnel::MAX_STREAM_WINDOW_SIZE`), the in-flight ceiling.
const PRODUCT_WINDOW: u32 = 4 * 1024 * 1024;

/// CPU time (user + system, all threads) this process has used so far, in microseconds. The
/// harness runs both tunnel ends, the loopback shaper and the test app in one process, so this is
/// an upper bound on the relay's own cost: the arm-to-arm delta at the same bottleneck rate is the
/// measurement, and the per-MB figures divide by the bytes the arm handed to the engine.
#[cfg(unix)]
fn process_cpu_us() -> u64 {
    // SAFETY: `usage` is a zeroed `rusage` that `getrusage` fills for this process.
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut usage) != 0 {
            return 0;
        }
        let user = usage.ru_utime.tv_sec as u64 * 1_000_000 + usage.ru_utime.tv_usec as u64;
        let sys = usage.ru_stime.tv_sec as u64 * 1_000_000 + usage.ru_stime.tv_usec as u64;
        user + sys
    }
}

#[cfg(not(unix))]
fn process_cpu_us() -> u64 {
    0
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Summary {
    pub scenario: &'static str,
    /// Which in-flight bound the run used (ADR-0048); `baseline` is today's relay.
    pub mechanism: String,
    pub evidence: &'static str,
    pub samples: usize,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
    pub bulk_bytes: u64,
    /// Whole-process CPU (user + system) spent while the scenario ran, in microseconds. Includes
    /// the harness itself, so it is an upper bound; see `process_cpu_us`.
    pub cpu_us: u64,
    /// True when the interactive ping did not return inside the arm's deadline. An arm that
    /// stalls its interactive flow is reported, not hidden: the harness never hangs the suite.
    pub stalled: bool,
}

impl Summary {
    pub(crate) fn json(&self, rate_bytes_per_s: u64) -> String {
        format!(
            "{{\"scenario\":\"{}\",\"mechanism\":\"{}\",\"evidence\":\"{}\",\"rate_bytes_per_s\":{},\"samples\":{},\"rtt_us\":{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}},\"bulk_bytes\":{},\"cpu_us\":{},\"stalled\":{}}}",
            self.scenario,
            self.mechanism,
            self.evidence,
            rate_bytes_per_s,
            self.samples,
            self.p50_us,
            self.p95_us,
            self.p99_us,
            self.max_us,
            self.bulk_bytes,
            self.cpu_us,
            self.stalled
        )
    }
}

pub(crate) fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Bottleneck profile. `aged` models a path that throttles long-lived flows (LAB run
/// 44056840174: flows older than about 60 s ran at about 21 KB/s): after `after` the rate drops
/// to `aged_rate`. Simulated; the thresholds here are scaled down so tests stay short.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Profile {
    pub rate: u64,
    pub aged: Option<(Duration, u64)>,
}

impl Profile {
    pub(crate) fn fixed(rate: u64) -> Self {
        Self { rate, aged: None }
    }

    fn rate_at(&self, age: Duration) -> u64 {
        match self.aged {
            Some((after, aged_rate)) if age >= after => aged_rate,
            _ => self.rate,
        }
    }
}

/// The in-flight bound under test (ADR-0048 decision 4). Each variant is applied only in the
/// harness; the product relay is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mechanism {
    /// Today's relay on both ends: no explicit in-flight bound.
    Baseline,
    /// M1: the bulk sender may have at most `budget` bytes unacknowledged in the Yamux stream,
    /// gated on the stream's remaining send credit (`send_window`).
    CreditBudget { budget: usize },
    /// M2: the receiving sink consumes in `budget`-sized pieces and waits one bottleneck
    /// serialization time after each piece, so credit is released at the bottleneck rate instead
    /// of the loopback rate.
    ReceiverPacing { budget: usize },
    /// M3: small socket send buffer and `TCP_NOTSENT_LOWAT` on the tunnel sockets (unprivileged).
    SocketBackpressure {
        send_buffer: usize,
        notsent_lowat: usize,
    },
    /// The bound measured by the TP-0.1 research (research section 9.2): the Yamux per-stream
    /// window itself. It is not M1/M2/M3; it is the reference arm that shows where the queue can
    /// actually be bounded, and it is what decision 4's bound is implemented as.
    WindowCap { bytes: u32 },
    /// The shipped `sched` switch (ADR-0048 decision 5): the window bound plus one DRR arbiter per
    /// relay direction. Measured against `WindowCap` with the same window.
    #[cfg(feature = "policy-pack")]
    Scheduler {
        window: u32,
        quantum: usize,
        cap: usize,
    },
    /// ADR-0056: the shipped scheduler arm plus the adaptive loop, starting from `profile`'s row
    /// and free to select any of the five accepted rows from the pack's own counters. Compared
    /// with `Scheduler` at the same starting row (`Mechanism::for_profile`).
    #[cfg(feature = "policy-pack")]
    Adaptive {
        window: u32,
        profile: crate::policy_profile::PolicyProfile,
    },
}

impl Mechanism {
    pub(crate) fn label(self) -> String {
        match self {
            Mechanism::Baseline => "baseline".to_string(),
            Mechanism::CreditBudget { budget } => format!("m1-credit-budget-{budget}"),
            Mechanism::ReceiverPacing { budget } => format!("m2-receive-pacing-{budget}"),
            Mechanism::SocketBackpressure {
                send_buffer,
                notsent_lowat,
            } => format!("m3-socket-backpressure-{send_buffer}-{notsent_lowat}"),
            Mechanism::WindowCap { bytes } => format!("window-cap-{bytes}"),
            #[cfg(feature = "policy-pack")]
            Mechanism::Scheduler {
                window,
                quantum,
                cap,
            } => format!("sched-w{window}-q{quantum}-cap{cap}"),
            #[cfg(feature = "policy-pack")]
            Mechanism::Adaptive { window, profile } => {
                format!("adaptive-w{window}-from-{}", profile.id())
            }
        }
    }
}

/// The TP-3.2 evidence arms are the shipped profiles, not copies of their numbers: this reads the
/// same table `policy_relay` resolves a configured `policy.profile` from, so an evidence record
/// cannot drift from the code it claims to measure (ADR-0031 decisions 15 and 19).
#[cfg(feature = "policy-pack")]
impl Mechanism {
    pub(crate) fn for_profile(profile: crate::policy_profile::PolicyProfile) -> Self {
        Mechanism::Scheduler {
            window: profile.window_bytes(),
            quantum: profile.quantum_bytes(),
            cap: profile.max_queued_bytes(),
        }
    }

    /// The same starting row under ADR-0056's adaptive loop: the control against `for_profile`.
    pub(crate) fn adaptive_from(profile: crate::policy_profile::PolicyProfile) -> Self {
        Mechanism::Adaptive {
            window: profile.window_bytes(),
            profile,
        }
    }
}

/// The Yamux per-stream window an arm runs with: the arm's own cap, or the product value.
fn window_for(mechanism: Mechanism) -> Option<u32> {
    match mechanism {
        Mechanism::WindowCap { bytes } => Some(bytes),
        #[cfg(feature = "policy-pack")]
        Mechanism::Scheduler { window, .. } | Mechanism::Adaptive { window, .. } => Some(window),
        _ => None,
    }
}

/// One end's scheduler session for the arm, if the arm selects the shipped `sched` switch.
fn policy_for(_mechanism: Mechanism) -> Option<ArmPolicy> {
    #[cfg(feature = "policy-pack")]
    if let Mechanism::Scheduler { quantum, cap, .. } = _mechanism {
        return Some(crate::policy_relay::PolicySession::with_config(
            crate::policy_sched::SchedulerConfig {
                quantum_bytes: quantum,
                max_queued_bytes: cap,
            },
        ));
    }
    #[cfg(feature = "policy-pack")]
    if let Mechanism::Adaptive { profile, .. } = _mechanism {
        return Some(crate::policy_relay::PolicySession::new(
            crate::policy_sched::SchedulerConfig {
                quantum_bytes: profile.quantum_bytes(),
                max_queued_bytes: profile.max_queued_bytes(),
            },
            profile,
            true,
        ));
    }
    None
}

/// Relay one direction pair for an arm: the scheduler when the arm has one, today's pump otherwise.
async fn relay_under<A, B>(peer: A, local: B, policy: Option<&ArmPolicy>) -> std::io::Result<()>
where
    A: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    B: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    #[cfg(feature = "policy-pack")]
    if let Some(policy) = policy {
        return crate::policy_relay::relay_with_policy(peer, local, IDLE, policy).await;
    }
    let _ = policy;
    relay_with_idle_timeout(peer, local, IDLE).await
}

/// Forward bytes no faster than the profile's rate for the connection's age (serialization
/// delay only; no propagation delay, loss or jitter).
async fn shaped_pump<R, W>(mut src: R, mut dst: W, profile: Profile, born: Instant)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buffer = vec![0_u8; PUMP_READ];
    loop {
        let Ok(count) = src.read(&mut buffer).await else {
            return;
        };
        if count == 0 {
            let _ = dst.shutdown().await;
            return;
        }
        let rate = profile.rate_at(born.elapsed());
        tokio::time::sleep(Duration::from_secs_f64(count as f64 / rate as f64)).await;
        if dst.write_all(&buffer[..count]).await.is_err() {
            return;
        }
    }
}

/// M3: clamp the kernel socket queues. `SO_SNDBUF` disables send-buffer autotuning on this
/// socket; `TCP_NOTSENT_LOWAT` wakes writability only when unsent bytes fall below the mark.
fn apply_socket_backpressure(stream: &TcpStream, send_buffer: usize, notsent_lowat: usize) {
    use std::os::fd::AsRawFd;
    let fd = stream.as_raw_fd();
    let size = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    for (level, option, value) in [
        (
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            send_buffer as libc::c_int,
        ),
        (
            libc::IPPROTO_TCP,
            libc::TCP_NOTSENT_LOWAT,
            notsent_lowat as libc::c_int,
        ),
    ] {
        // SAFETY: `fd` is a live TCP socket owned by `stream` and `value` outlives the call.
        unsafe {
            libc::setsockopt(
                fd,
                level,
                option,
                &value as *const libc::c_int as *const libc::c_void,
                size,
            );
        }
    }
}

/// Two loopback TCP endpoints joined by a bottleneck proxy of `rate` bytes/s each way.
async fn shaped_endpoints(profile: Profile) -> (TcpStream, TcpStream) {
    shaped_endpoints_with(profile, Mechanism::Baseline).await
}

async fn shaped_endpoints_with(profile: Profile, mechanism: Mechanism) -> (TcpStream, TcpStream) {
    let born = Instant::now();
    let server_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind server");
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
    let server_addr = server_listener.local_addr().expect("server addr");
    let proxy_addr = proxy_listener.local_addr().expect("proxy addr");
    let accept_server =
        tokio::spawn(async move { server_listener.accept().await.expect("accept").0 });
    tokio::spawn(async move {
        let (inbound, _) = proxy_listener.accept().await.expect("proxy accept");
        let outbound = TcpStream::connect(server_addr).await.expect("proxy dial");
        let (in_read, in_write) = inbound.into_split();
        let (out_read, out_write) = outbound.into_split();
        tokio::spawn(shaped_pump(in_read, out_write, profile, born));
        tokio::spawn(shaped_pump(out_read, in_write, profile, born));
    });
    let client = TcpStream::connect(proxy_addr).await.expect("client dial");
    let server = accept_server.await.expect("server accept task");
    let _ = (client.set_nodelay(true), server.set_nodelay(true));
    if let Mechanism::SocketBackpressure {
        send_buffer,
        notsent_lowat,
    } = mechanism
    {
        apply_socket_backpressure(&client, send_buffer, notsent_lowat);
        apply_socket_backpressure(&server, send_buffer, notsent_lowat);
    }
    (client, server)
}

/// M1: relay one flow from the local app into the Yamux stream, keeping at most `budget` bytes
/// unacknowledged. `window` is the session's per-stream maximum: it is the credit the peer grants
/// once at stream creation (initial window plus one update), so `window - send_window()` is exactly
/// the bytes written and not yet acknowledged, with no delta bookkeeping to get wrong.
async fn credit_relay(
    mut peer: tokio_yamux::StreamHandle,
    mut local: DuplexStream,
    budget: usize,
    window: u32,
) -> std::io::Result<()> {
    let reference = window as usize;
    let mut buffer = vec![0_u8; PUMP_READ];
    loop {
        let credit = peer.send_window() as usize;
        let outstanding = reference.saturating_sub(credit);
        if outstanding >= budget {
            // The flow is at its in-flight bound. Poll the stream so window updates are applied:
            // this harness direction carries no return traffic, and `send_window` only grows when
            // the handle drains its frames. A closed or failed stream must end the relay; reading
            // it again would spin without an await point and starve the test runtime.
            let mut scratch = [0_u8; 64];
            match tokio::time::timeout(Duration::from_millis(1), peer.read(&mut scratch)).await {
                Ok(Ok(0)) | Ok(Err(_)) => return Ok(()),
                _ => continue,
            }
        }
        let room = (budget - outstanding).min(buffer.len());
        let count = match local.read(&mut buffer[..room]).await {
            Ok(0) => {
                let _ = peer.shutdown().await;
                return Ok(());
            }
            Err(_) => return Ok(()),
            Ok(count) => count,
        };
        peer.write_all(&buffer[..count]).await?;
        let _ = peer.flush().await;
    }
}

/// M2: consume a receiving sink in `budget` pieces, one bottleneck serialization time apart.
async fn paced_sink<R: AsyncRead + Unpin>(mut read: R, budget: usize, rate: u64) {
    let mut buffer = vec![0_u8; budget];
    loop {
        match read.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                tokio::time::sleep(Duration::from_secs_f64(count as f64 / rate as f64)).await;
            }
        }
    }
}

/// One scenario: an interactive ping stream, optionally beside a bulk uploader. The first stream
/// the client opens in a bulk scenario is the bulk one (the server sinks it); the next echoes.
pub(crate) async fn run_scenario(
    scenario: &'static str,
    with_bulk: bool,
    rate: u64,
    duration: Duration,
    interval: Duration,
) -> Summary {
    run_scenario_window(scenario, with_bulk, rate, duration, interval, None).await
}

/// Experiment hook (ADR-0048, mechanism "window cap"): same scenario with the Yamux per-stream
/// maximum window overridden. `None` keeps the product value.
fn config_with_window(window: Option<u32>) -> tokio_yamux::Config {
    let mut config = yamux_config(8);
    if let Some(window) = window {
        config.max_stream_window_size = window;
    }
    config
}

pub(crate) async fn run_scenario_window(
    scenario: &'static str,
    with_bulk: bool,
    rate: u64,
    duration: Duration,
    interval: Duration,
    window: Option<u32>,
) -> Summary {
    run_variant(
        scenario,
        with_bulk,
        rate,
        duration,
        interval,
        window,
        Mechanism::Baseline,
    )
    .await
}

/// Same scenario with one ADR-0048 mechanism variant applied to the bulk flow.
pub(crate) async fn run_scenario_mechanism(
    scenario: &'static str,
    with_bulk: bool,
    rate: u64,
    duration: Duration,
    interval: Duration,
    mechanism: Mechanism,
) -> Summary {
    run_variant(
        scenario, with_bulk, rate, duration, interval, None, mechanism,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_variant(
    scenario: &'static str,
    with_bulk: bool,
    rate: u64,
    duration: Duration,
    interval: Duration,
    window: Option<u32>,
    mechanism: Mechanism,
) -> Summary {
    let (client_io, server_io) = shaped_endpoints_with(Profile::fixed(rate), mechanism).await;
    let cpu_start = process_cpu_us();
    let window = window.or(window_for(mechanism));
    let mut server_session = Session::new_server(server_io, config_with_window(window));
    let sink_mechanism = mechanism;
    // Each tunnel end owns its scheduler, as two real hosts would.
    let server_policy = policy_for(mechanism);
    let server_task = tokio::spawn(async move {
        let mut index = 0_u32;
        while let Some(item) = server_session.next().await {
            let Ok(stream) = item else { break };
            let is_sink = with_bulk && index == 0;
            index += 1;
            let relay_policy = server_policy.clone();
            tokio::spawn(async move {
                let (local, app) = duplex(64 * 1024);
                tokio::spawn(async move {
                    let _ = relay_under(stream, local, relay_policy.as_ref()).await;
                });
                let (mut read, mut write) = tokio::io::split(app);
                if is_sink {
                    match sink_mechanism {
                        Mechanism::ReceiverPacing { budget } => {
                            paced_sink(read, budget, rate).await
                        }
                        _ => {
                            let mut buffer = vec![0_u8; PUMP_READ];
                            while let Ok(count) = read.read(&mut buffer).await {
                                if count == 0 {
                                    return;
                                }
                            }
                        }
                    }
                } else {
                    let mut ping = [0_u8; PING];
                    while read.read_exact(&mut ping).await.is_ok() {
                        if write.write_all(&ping).await.is_err() {
                            return;
                        }
                    }
                }
            });
        }
    });
    let mut client_session = Session::new_client(client_io, config_with_window(window));
    let control = client_session.control();
    let client_task = tokio::spawn(async move {
        while let Some(item) = client_session.next().await {
            if item.is_err() {
                break;
            }
        }
    });

    let client_policy = policy_for(mechanism);
    let open = |mut control: tokio_yamux::Control,
                mechanism: Mechanism,
                policy: Option<ArmPolicy>| async move {
        let stream = control.open_stream().await.expect("open stream");
        let (local, app): (DuplexStream, DuplexStream) = duplex(64 * 1024);
        match mechanism {
            Mechanism::CreditBudget { budget } => {
                tokio::spawn(async move {
                    let _ = credit_relay(stream, local, budget, PRODUCT_WINDOW).await;
                });
            }
            _ => {
                tokio::spawn(async move {
                    let _ = relay_under(stream, local, policy.as_ref()).await;
                });
            }
        }
        app
    };

    let bulk_sent = Arc::new(AtomicU64::new(0));
    let bulk_task = if with_bulk {
        let mut app = open(control.clone(), mechanism, client_policy.clone()).await;
        let sent = Arc::clone(&bulk_sent);
        Some(tokio::spawn(async move {
            let payload = vec![0x5a_u8; PUMP_READ];
            while app.write_all(&payload).await.is_ok() {
                sent.fetch_add(payload.len() as u64, Ordering::Relaxed);
            }
        }))
    } else {
        None
    };
    if with_bulk {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let mut interactive = open(control.clone(), Mechanism::Baseline, client_policy.clone()).await;
    let ping = [7_u8; PING];
    let mut echo = [0_u8; PING];
    let mut samples = Vec::new();
    let end = Instant::now() + duration;
    // One ping must never outlive the arm: an arm that wedges its interactive flow is recorded as
    // stalled instead of hanging the measurement run.
    let deadline = duration + Duration::from_secs(10);
    let stalled = tokio::time::timeout(deadline, async {
        while Instant::now() < end {
            let start = Instant::now();
            interactive.write_all(&ping).await.expect("ping");
            interactive.read_exact(&mut echo).await.expect("echo");
            samples.push(start.elapsed().as_micros() as u64);
            tokio::time::sleep(interval).await;
        }
    })
    .await
    .is_err();

    if let Some(task) = bulk_task {
        task.abort();
    }
    let bulk_bytes = bulk_sent.load(Ordering::Relaxed);
    server_task.abort();
    client_task.abort();
    let cpu_us = process_cpu_us().saturating_sub(cpu_start);
    samples.sort_unstable();
    Summary {
        scenario,
        mechanism: mechanism.label(),
        evidence: "simulated",
        samples: samples.len(),
        p50_us: percentile(&samples, 50.0),
        p95_us: percentile(&samples, 95.0),
        p99_us: percentile(&samples, 99.0),
        max_us: samples.last().copied().unwrap_or(0),
        bulk_bytes,
        cpu_us,
        stalled,
    }
}

/// Time to push `bytes` through one stream (acknowledged by the receiver with one byte) after the
/// connection has existed for `wait`. Models the aged-session throttle: compare `wait` below and
/// above the profile's threshold. Returns elapsed milliseconds.
pub(crate) async fn run_transfer(profile: Profile, wait: Duration, bytes: usize) -> u64 {
    run_transfer_mechanism(profile, wait, bytes, Mechanism::Baseline).await
}

/// `run_transfer` with one ADR-0048 mechanism applied to the transfer's relay.
pub(crate) async fn run_transfer_mechanism(
    profile: Profile,
    wait: Duration,
    bytes: usize,
    mechanism: Mechanism,
) -> u64 {
    let (client_io, server_io) = shaped_endpoints_with(profile, mechanism).await;
    let config = || config_with_window(window_for(mechanism));
    let mut server_session = Session::new_server(server_io, config());
    let server_policy = policy_for(mechanism);
    let server_task = tokio::spawn(async move {
        while let Some(item) = server_session.next().await {
            let Ok(stream) = item else { break };
            let relay_policy = server_policy.clone();
            tokio::spawn(async move {
                let (local, app) = duplex(64 * 1024);
                tokio::spawn(async move {
                    let _ = relay_under(stream, local, relay_policy.as_ref()).await;
                });
                let (mut read, mut write) = tokio::io::split(app);
                let mut remaining = bytes;
                let mut buffer = vec![0_u8; PUMP_READ];
                while remaining > 0 {
                    match read.read(&mut buffer).await {
                        Ok(0) | Err(_) => return,
                        Ok(count) => remaining = remaining.saturating_sub(count),
                    }
                }
                let _ = write.write_all(&[1]).await;
            });
        }
    });
    let mut client_session = Session::new_client(client_io, config());
    let mut control = client_session.control();
    let client_task = tokio::spawn(async move {
        while let Some(item) = client_session.next().await {
            if item.is_err() {
                break;
            }
        }
    });
    tokio::time::sleep(wait).await;
    let stream = control.open_stream().await.expect("open stream");
    let (local, mut app): (DuplexStream, DuplexStream) = duplex(64 * 1024);
    let client_policy = policy_for(mechanism);
    match mechanism {
        Mechanism::CreditBudget { budget } => {
            tokio::spawn(async move {
                let _ = credit_relay(stream, local, budget, PRODUCT_WINDOW).await;
            });
        }
        _ => {
            tokio::spawn(async move {
                let _ = relay_under(stream, local, client_policy.as_ref()).await;
            });
        }
    }
    let start = Instant::now();
    let payload = vec![0x5a_u8; PUMP_READ];
    let mut sent = 0;
    while sent < bytes {
        let n = payload.len().min(bytes - sent);
        app.write_all(&payload[..n]).await.expect("write");
        sent += n;
    }
    let mut ack = [0_u8; 1];
    app.read_exact(&mut ack).await.expect("ack");
    let elapsed = start.elapsed().as_millis() as u64;
    server_task.abort();
    client_task.abort();
    elapsed
}

/// What the ADR-0056 loop did during one multi-flow run. Only `Adaptive` arms fill it; every other
/// arm reports `None`, so their records are unchanged.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct AdaptiveReport {
    pub changes: usize,
    /// `<direction>:<from>-><to>` in order.
    pub transitions: Vec<String>,
    /// When each change was accepted, in milliseconds since the session's driver started.
    pub change_at_ms: Vec<u64>,
    /// Gaps between consecutive changes, in milliseconds (dwell and rate limit show up here).
    pub gaps_ms: Vec<u64>,
    pub estimates: usize,
    pub estimate_p50_ms: u64,
    pub estimate_p95_ms: u64,
    pub estimate_max_ms: u64,
    /// Samples whose estimate was above the ADR's 50 ms down-step threshold.
    pub estimate_over_50ms: usize,
}

impl AdaptiveReport {
    fn json(&self) -> String {
        let list = |values: &[u64]| {
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        let transitions = self
            .transitions
            .iter()
            .map(|transition| format!("\"{transition}\""))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"changes\":{},\"transitions\":[{}],\"change_at_ms\":[{}],\"gaps_ms\":[{}],\"estimates\":{},\"estimate_p50_ms\":{},\"estimate_p95_ms\":{},\"estimate_max_ms\":{},\"estimate_over_50ms\":{}}}",
            self.changes,
            transitions,
            list(&self.change_at_ms),
            list(&self.gaps_ms),
            self.estimates,
            self.estimate_p50_ms,
            self.estimate_p95_ms,
            self.estimate_max_ms,
            self.estimate_over_50ms
        )
    }

    #[cfg(feature = "policy-pack")]
    fn from_session(session: &crate::policy_relay::PolicySession) -> Option<Self> {
        let changes = session.changes();
        let mut estimates: Vec<u64> = session
            .estimates()
            .into_iter()
            .map(|(_, estimate)| estimate.as_millis() as u64)
            .collect();
        if changes.is_empty() && estimates.is_empty() {
            return None;
        }
        estimates.sort_unstable();
        let mut report = AdaptiveReport {
            changes: changes.len(),
            transitions: changes
                .iter()
                .map(|change| {
                    let direction = match change.direction {
                        Direction::ToPeer => "to_peer",
                        Direction::FromPeer => "from_peer",
                    };
                    format!("{direction}:{}->{}", change.change.from, change.change.to)
                })
                .collect(),
            change_at_ms: changes
                .iter()
                .map(|change| change.change.at.as_millis() as u64)
                .collect(),
            estimates: estimates.len(),
            estimate_p50_ms: percentile(&estimates, 50.0),
            estimate_p95_ms: percentile(&estimates, 95.0),
            estimate_max_ms: estimates.last().copied().unwrap_or(0),
            estimate_over_50ms: estimates.iter().filter(|value| **value > 50).count(),
            gaps_ms: Vec::new(),
        };
        report.gaps_ms = report
            .change_at_ms
            .windows(2)
            .map(|pair| pair[1].saturating_sub(pair[0]))
            .collect();
        Some(report)
    }
}

/// Multi-flow arm result (TP-2.2). Several bulk flows compete for one shaped bottleneck, each with
/// its own echo stream, so arbitration between flows is measured instead of inferred from the
/// single bulk + single interactive pair.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MultiFlowSummary {
    pub scenario: &'static str,
    pub mechanism: String,
    pub evidence: &'static str,
    pub flows: usize,
    /// Bytes each bulk flow handed to its relay.
    pub bulk_bytes: Vec<u64>,
    /// Per-flow interactive RTT, in microseconds: flow `i`'s echo stream beside flow `i`'s bulk.
    pub flow_p50_us: Vec<u64>,
    pub flow_p95_us: Vec<u64>,
    /// Whole-process CPU (upper bound, see `process_cpu_us`).
    pub cpu_us: u64,
    /// `Some(0)` when the churn variant closed flow 0 at half the run.
    pub churn_closed: Option<usize>,
    /// True when every flow that was not churned still handed bytes after the churn.
    pub continued_after_churn: bool,
    pub stalled: bool,
    /// ADR-0056: what the loop did on the client session, `None` for every static arm.
    pub adaptive: Option<AdaptiveReport>,
}

impl MultiFlowSummary {
    pub(crate) fn total_bulk_bytes(&self) -> u64 {
        self.bulk_bytes.iter().sum()
    }

    /// Smallest share divided by the largest, over the flows that ran the whole time (a churned
    /// flow stopped by design and is excluded). 1.0 is an even split, 0.0 means a flow was starved.
    pub(crate) fn delivery_min_over_max(&self) -> f64 {
        let first = match self.churn_closed {
            Some(index) => index + 1,
            None => 0,
        };
        let shares = &self.bulk_bytes[first.min(self.bulk_bytes.len())..];
        let max = shares.iter().copied().max().unwrap_or(0);
        if max == 0 {
            return 0.0;
        }
        shares.iter().copied().min().unwrap_or(0) as f64 / max as f64
    }

    pub(crate) fn json(&self) -> String {
        let list = |values: &[u64]| {
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        format!(
            "{{\"scenario\":\"{}\",\"mechanism\":\"{}\",\"evidence\":\"{}\",\"flows\":{},\"bulk_bytes\":[{}],\"total_bulk_bytes\":{},\"delivery_min_over_max\":{:.3},\"flow_p50_us\":[{}],\"flow_p95_us\":[{}],\"cpu_us\":{},\"churn_closed\":{},\"continued_after_churn\":{},\"stalled\":{},\"adaptive\":{}}}",
            self.scenario,
            self.mechanism,
            self.evidence,
            self.flows,
            list(&self.bulk_bytes),
            self.total_bulk_bytes(),
            self.delivery_min_over_max(),
            list(&self.flow_p50_us),
            list(&self.flow_p95_us),
            self.cpu_us,
            self.churn_closed
                .map(|index| index.to_string())
                .unwrap_or_else(|| "null".to_string()),
            self.continued_after_churn,
            self.stalled,
            self.adaptive
                .as_ref()
                .map(AdaptiveReport::json)
                .unwrap_or_else(|| "null".to_string())
        )
    }
}

/// Multi-flow scenario (TP-2.2): `flows` bulk uploaders share the bottleneck, plus one 64-byte echo
/// stream per flow for per-flow latency. `churn` closes flow 0's sender at half the run and records
/// whether the surviving flows kept moving.
#[allow(clippy::too_many_arguments)]
async fn run_multi_flow(
    scenario: &'static str,
    flows: usize,
    churn: bool,
    rate: u64,
    duration: Duration,
    interval: Duration,
    mechanism: Mechanism,
) -> MultiFlowSummary {
    let flows = flows.max(1);
    let (client_io, server_io) = shaped_endpoints_with(Profile::fixed(rate), mechanism).await;
    let cpu_start = process_cpu_us();
    let window = window_for(mechanism);
    let mut server_session = Session::new_server(server_io, config_with_window(window));
    let server_policy = policy_for(mechanism);
    // Streams arrive in order: the first `flows` are bulk sinks, the next `flows` echo pings.
    let server_task = tokio::spawn(async move {
        let mut index = 0_usize;
        while let Some(item) = server_session.next().await {
            let Ok(stream) = item else { break };
            let is_sink = index < flows;
            index += 1;
            let relay_policy = server_policy.clone();
            tokio::spawn(async move {
                let (local, app) = duplex(64 * 1024);
                tokio::spawn(async move {
                    let _ = relay_under(stream, local, relay_policy.as_ref()).await;
                });
                let (mut read, mut write) = tokio::io::split(app);
                if is_sink {
                    let mut buffer = vec![0_u8; PUMP_READ];
                    while let Ok(count) = read.read(&mut buffer).await {
                        if count == 0 {
                            return;
                        }
                    }
                } else {
                    let mut ping = [0_u8; PING];
                    while read.read_exact(&mut ping).await.is_ok() {
                        if write.write_all(&ping).await.is_err() {
                            return;
                        }
                    }
                }
            });
        }
    });
    let mut client_session = Session::new_client(client_io, config_with_window(window));
    let control = client_session.control();
    let client_task = tokio::spawn(async move {
        while let Some(item) = client_session.next().await {
            if item.is_err() {
                break;
            }
        }
    });

    let client_policy = policy_for(mechanism);
    let open = |mut control: tokio_yamux::Control, policy: Option<ArmPolicy>| async move {
        let stream = control.open_stream().await.expect("open stream");
        let (local, app): (DuplexStream, DuplexStream) = duplex(64 * 1024);
        tokio::spawn(async move {
            let _ = relay_under(stream, local, policy.as_ref()).await;
        });
        app
    };

    let counters: Vec<Arc<AtomicU64>> = (0..flows).map(|_| Arc::new(AtomicU64::new(0))).collect();
    let mut apps = Vec::with_capacity(flows);
    for _ in 0..flows {
        apps.push(open(control.clone(), client_policy.clone()).await);
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    let start = Instant::now();
    let end = start + duration;
    let churn_at = churn.then(|| start + duration / 2);
    let mut bulk_tasks = Vec::with_capacity(flows);
    for (index, mut app) in apps.into_iter().enumerate() {
        let counter = Arc::clone(&counters[index]);
        let close_at = if index == 0 { churn_at } else { None };
        bulk_tasks.push(tokio::spawn(async move {
            let payload = vec![0x5a_u8; PUMP_READ];
            loop {
                match close_at {
                    Some(at) => {
                        tokio::select! {
                            result = app.write_all(&payload) => {
                                if result.is_err() {
                                    return;
                                }
                            }
                            _ = tokio::time::sleep_until(at.into()) => {
                                let _ = app.shutdown().await;
                                return;
                            }
                        }
                    }
                    None => {
                        if app.write_all(&payload).await.is_err() {
                            return;
                        }
                    }
                }
                counter.fetch_add(payload.len() as u64, Ordering::Relaxed);
            }
        }));
    }

    let mut ping_set = tokio::task::JoinSet::new();
    for _ in 0..flows {
        let mut app = open(control.clone(), client_policy.clone()).await;
        ping_set.spawn(async move {
            let mut samples = Vec::new();
            let ping = [7_u8; PING];
            let mut echo = [0_u8; PING];
            while Instant::now() < end {
                let asked = Instant::now();
                if app.write_all(&ping).await.is_err() {
                    break;
                }
                if app.read_exact(&mut echo).await.is_err() {
                    break;
                }
                samples.push(asked.elapsed().as_micros() as u64);
                tokio::time::sleep(interval).await;
            }
            samples
        });
    }

    let churn_snapshot: Vec<u64> = match churn_at {
        Some(at) => {
            tokio::time::sleep_until(at.into()).await;
            tokio::time::sleep(Duration::from_millis(300)).await;
            counters
                .iter()
                .map(|counter| counter.load(Ordering::Relaxed))
                .collect()
        }
        None => Vec::new(),
    };

    let grace = (end + Duration::from_secs(10)).saturating_duration_since(Instant::now());
    let mut per_flow_samples: Vec<Vec<u64>> = Vec::new();
    let stalled = {
        let collect = async {
            while let Some(result) = ping_set.join_next().await {
                per_flow_samples.push(result.unwrap_or_default());
            }
        };
        tokio::time::timeout(grace, collect).await.is_err()
    };
    if stalled {
        ping_set.abort_all();
    }

    let bulk_bytes = counters
        .iter()
        .map(|counter| counter.load(Ordering::Relaxed))
        .collect::<Vec<_>>();
    let continued_after_churn = match churn_at {
        Some(_) => (1..flows)
            .all(|index| bulk_bytes[index] > churn_snapshot.get(index).copied().unwrap_or(0)),
        None => true,
    };
    for task in bulk_tasks {
        task.abort();
    }
    server_task.abort();
    client_task.abort();
    let cpu_us = process_cpu_us().saturating_sub(cpu_start);

    let mut flow_p50_us = Vec::with_capacity(per_flow_samples.len());
    let mut flow_p95_us = Vec::with_capacity(per_flow_samples.len());
    for samples in per_flow_samples.iter_mut() {
        samples.sort_unstable();
        flow_p50_us.push(percentile(samples, 50.0));
        flow_p95_us.push(percentile(samples, 95.0));
    }
    #[cfg(feature = "policy-pack")]
    let adaptive = client_policy
        .as_ref()
        .map(|policy| policy.as_ref())
        .and_then(AdaptiveReport::from_session);
    #[cfg(not(feature = "policy-pack"))]
    let adaptive = None;
    MultiFlowSummary {
        scenario,
        mechanism: mechanism.label(),
        evidence: "simulated",
        flows,
        bulk_bytes,
        flow_p50_us,
        flow_p95_us,
        cpu_us,
        churn_closed: churn.then_some(0),
        continued_after_churn,
        stalled,
        adaptive,
    }
}

/// Two loopback TCP endpoints with nothing in between: the relay CPU probe's path. No bottleneck
/// and no sleeps, so process CPU is the relay's own cost per byte relayed.
async fn direct_endpoints() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let accept = tokio::spawn(async move { listener.accept().await.expect("accept").0 });
    let client = TcpStream::connect(addr).await.expect("dial");
    let server = accept.await.expect("accept task");
    let _ = (client.set_nodelay(true), server.set_nodelay(true));
    (client, server)
}

/// Per-MB CPU of one arm on an unshaped loopback session: `bytes` pushed through one stream with
/// both ends in the process and no rate limiter. Returns `(cpu_us, delivered_bytes)`. This is the
/// figure ADR-0048 decision 6's overhead budget is about: the relay's own cost per byte, without
/// the shaper's constant work in the denominator.
async fn relay_cpu_probe(mechanism: Mechanism, bytes: u64) -> (u64, u64) {
    let (client_io, server_io) = direct_endpoints().await;
    let window = window_for(mechanism);
    let mut server_session = Session::new_server(server_io, config_with_window(window));
    let server_policy = policy_for(mechanism);
    let delivered = Arc::new(AtomicU64::new(0));
    let sink_total = Arc::clone(&delivered);
    let server_task = tokio::spawn(async move {
        while let Some(item) = server_session.next().await {
            let Ok(stream) = item else { break };
            let relay_policy = server_policy.clone();
            let sink_total = Arc::clone(&sink_total);
            tokio::spawn(async move {
                let (local, mut app) = duplex(64 * 1024);
                tokio::spawn(async move {
                    let _ = relay_under(stream, local, relay_policy.as_ref()).await;
                });
                let mut buffer = vec![0_u8; PUMP_READ];
                let mut total = 0_u64;
                while let Ok(count) = app.read(&mut buffer).await {
                    if count == 0 {
                        break;
                    }
                    total += count as u64;
                }
                sink_total.store(total, Ordering::SeqCst);
            });
        }
    });
    let mut client_session = Session::new_client(client_io, config_with_window(window));
    let mut control = client_session.control();
    let client_task = tokio::spawn(async move {
        while let Some(item) = client_session.next().await {
            if item.is_err() {
                break;
            }
        }
    });
    let stream = control.open_stream().await.expect("open stream");
    let (local, mut app) = duplex(64 * 1024);
    let client_policy = policy_for(mechanism);
    tokio::spawn(async move {
        let _ = relay_under(stream, local, client_policy.as_ref()).await;
    });

    let cpu_start = process_cpu_us();
    let payload = vec![0x5a_u8; PUMP_READ];
    let mut sent = 0_u64;
    while sent < bytes {
        let count = payload.len().min((bytes - sent) as usize);
        app.write_all(&payload[..count]).await.expect("write");
        sent += count as u64;
    }
    let _ = app.shutdown().await;
    // Keep the CPU window open until the sink has drained the relayed bytes, so every arm's window
    // covers the same work; the bound keeps a broken arm from hanging the probe.
    let deadline = Instant::now() + Duration::from_secs(60);
    while delivered.load(Ordering::SeqCst) < bytes && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let cpu_us = process_cpu_us().saturating_sub(cpu_start);
    let total = delivered.load(Ordering::SeqCst);
    server_task.abort();
    client_task.abort();
    (cpu_us, total)
}

/// The arms of the ADR-0048 comparison: today's relay, the three candidate bounds, and the window
/// arm. M1 runs at half the product window because that is the smallest budget Yamux can credit:
/// a receiver emits a window update only once `max_recv_window / 2` bytes are consumed (see
/// `credit_budget_below_the_window_granularity_stalls`), so a smaller budget stalls the flow.
pub(crate) fn mechanism_matrix() -> Vec<Mechanism> {
    #[allow(unused_mut)]
    // `mut` is only needed with the feature, where the scheduler arm is pushed.
    let mut arms = vec![
        Mechanism::Baseline,
        Mechanism::CreditBudget {
            budget: PRODUCT_WINDOW as usize / 2,
        },
        Mechanism::ReceiverPacing { budget: 64 * 1024 },
        Mechanism::SocketBackpressure {
            send_buffer: 32 * 1024,
            notsent_lowat: 16 * 1024,
        },
        Mechanism::WindowCap { bytes: 256 * 1024 },
    ];
    // TP-2.2: the shipped scheduler on the same 256 KiB window as the arm above, so the comparison
    // isolates arbitration rather than the in-flight bound.
    #[cfg(feature = "policy-pack")]
    arms.push(Mechanism::Scheduler {
        window: 256 * 1024,
        quantum: 16 * 1024,
        cap: 64 * 1024,
    });
    arms
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "policy-pack")]
    use crate::policy_profile::PolicyProfile;

    const RATE: u64 = 2_500_000; // about 20 Mbit/s

    /// The comparison run's knobs: `POLICY_RATE`, `POLICY_REPEATS` and `POLICY_SECONDS`.
    fn experiment_settings(default_seconds: u64) -> (u64, usize, u64) {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .and_then(|value| value.parse().ok())
        };
        (
            read("POLICY_RATE").unwrap_or(RATE),
            read("POLICY_REPEATS").unwrap_or(2) as usize,
            read("POLICY_SECONDS").unwrap_or(default_seconds),
        )
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let data = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        assert_eq!(percentile(&data, 50.0), 50);
        assert_eq!(percentile(&data, 95.0), 100);
        assert_eq!(percentile(&[], 50.0), 0);
    }

    #[test]
    fn mechanism_labels_are_distinct_and_name_the_arm() {
        let labels: Vec<String> = mechanism_matrix().iter().map(|m| m.label()).collect();
        assert_eq!(labels[0], "baseline");
        assert!(labels[1].starts_with("m1-credit-budget-"));
        assert!(labels[2].starts_with("m2-receive-pacing-"));
        assert!(labels[3].starts_with("m3-socket-backpressure-"));
        assert_eq!(labels[4], "window-cap-262144");
        #[cfg(feature = "policy-pack")]
        assert_eq!(
            labels[5], "sched-w262144-q16384-cap65536",
            "the scheduler arm rides the same 256 KiB window as the arm it is compared with"
        );
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len());
    }

    /// TP-3.2: the evidence arms must be the shipped rows (ADR-0031 decision 19), so a record
    /// measures the profile the tunnel config resolves rather than a hand-copied set of numbers.
    #[cfg(feature = "policy-pack")]
    #[test]
    fn profile_arms_are_the_shipped_rows() {
        use crate::policy_profile::PolicyProfile;

        let balanced = Mechanism::for_profile(PolicyProfile::Balanced);
        assert_eq!(
            balanced.label(),
            "sched-w262144-q16384-cap65536",
            "balanced/1 is the identity and must reproduce the TP-2.2 scheduler arm"
        );
        let gaming = Mechanism::for_profile(PolicyProfile::Gaming);
        assert_eq!(gaming.label(), "sched-w262144-q4096-cap16384");
        assert_ne!(
            gaming, balanced,
            "gaming/1 must differ from the identity in quantum and cap"
        );
        let trading = Mechanism::for_profile(PolicyProfile::Trading);
        assert_eq!(
            trading.label(),
            "sched-w262144-q2048-cap8192",
            "trading/1 is the finest interleaving: its quantum and cap must be the row's"
        );
        assert_ne!(
            trading, gaming,
            "trading/1 must differ from gaming/1 in quantum and cap"
        );
        assert_ne!(trading, balanced);
        let streaming = Mechanism::for_profile(PolicyProfile::Streaming);
        assert_eq!(
            streaming.label(),
            "sched-w524288-q32768-cap131072",
            "streaming/1 is the row with twice Balanced's window and quantum"
        );
        assert_ne!(
            streaming, balanced,
            "streaming/1 must differ from the identity in window, quantum and cap"
        );
        assert_ne!(
            streaming, trading,
            "streaming/1 must not be confusable with the finest-interleaving row"
        );
        let bulk = Mechanism::for_profile(PolicyProfile::Bulk);
        assert_eq!(
            bulk.label(),
            "sched-w2097152-q65536-cap262144",
            "bulk/1 is the widest row: eight times Balanced's window"
        );
        assert_ne!(
            bulk, balanced,
            "bulk/1 must differ from the identity in window, quantum and cap"
        );
        assert_ne!(
            bulk, streaming,
            "bulk/1 must not be confusable with the half-width streaming row"
        );
        for profile in [
            PolicyProfile::Balanced,
            PolicyProfile::Gaming,
            PolicyProfile::Trading,
            PolicyProfile::Streaming,
            PolicyProfile::Bulk,
        ] {
            let arm = Mechanism::for_profile(profile);
            assert_eq!(
                window_for(arm),
                Some(profile.window_bytes()),
                "{} must run its row's window",
                profile.id()
            );
            match arm {
                Mechanism::Scheduler {
                    window,
                    quantum,
                    cap,
                } => assert_eq!(
                    (window, quantum, cap),
                    (
                        profile.window_bytes(),
                        profile.quantum_bytes(),
                        profile.max_queued_bytes()
                    ),
                    "{} must run exactly its row's numbers",
                    profile.id()
                ),
                other => panic!(
                    "{} must map to a scheduler arm, got {}",
                    profile.id(),
                    other.label()
                ),
            }
        }
    }

    /// TP-3.3: the row the record measures is the row a configured `trading/1` resolves to, and the
    /// two paths that must not change with it hold — no profile (today's numbers) and an unknown id
    /// (refused with the accepted values, never silently mapped to Balanced).
    #[cfg(feature = "policy-pack")]
    #[test]
    fn trading_profile_path_and_its_fallbacks_are_unchanged() {
        use crate::policy_relay::PolicyRelay;

        let resolved = PolicyRelay::resolve("sched", Some(PolicyProfile::Trading))
            .expect("a configured trading/1 runs the pack");
        assert_eq!(resolved.stream_window, 256 * 1024);
        assert_eq!(
            resolved.scheduler,
            Some(crate::policy_sched::SchedulerConfig {
                quantum_bytes: 2 * 1024,
                max_queued_bytes: 8 * 1024,
            })
        );
        assert_eq!(
            Mechanism::for_profile(PolicyProfile::Trading).label(),
            "sched-w262144-q2048-cap8192",
            "the evidence arm must be the row the config resolves"
        );
        // Absent profile: the switch's own numbers, exactly as before TP-3.1.
        assert_eq!(
            PolicyRelay::resolve("sched", None),
            PolicyRelay::parse("sched")
        );
        assert_eq!(PolicyRelay::resolve("", None), None);
        let error = PolicyProfile::parse("turbo/1").expect_err("turbo/1 must not parse");
        assert!(error.to_string().contains("trading/1"));
    }

    /// TP-3.4: the row the record measures is the row a configured `streaming/1` resolves to, and
    /// the two paths that must not change with it hold — no profile (today's numbers exactly) and
    /// an unknown id (refused naming the accepted values, never silently mapped to Balanced).
    #[cfg(feature = "policy-pack")]
    #[test]
    fn streaming_profile_path_and_its_fallbacks_are_unchanged() {
        use crate::policy_relay::PolicyRelay;

        let resolved = PolicyRelay::resolve("sched", Some(PolicyProfile::Streaming))
            .expect("a configured streaming/1 runs the pack");
        assert_eq!(resolved.stream_window, 512 * 1024);
        assert_eq!(
            resolved.scheduler,
            Some(crate::policy_sched::SchedulerConfig {
                quantum_bytes: 32 * 1024,
                max_queued_bytes: 128 * 1024,
            })
        );
        assert_eq!(
            Mechanism::for_profile(PolicyProfile::Streaming).label(),
            "sched-w524288-q32768-cap131072",
            "the evidence arm must be the row the config resolves"
        );
        // Absent profile: the switch's own numbers, exactly as before TP-3.1.
        assert_eq!(
            PolicyRelay::resolve("sched", None),
            PolicyRelay::parse("sched")
        );
        assert_eq!(PolicyRelay::resolve("", None), None);
        let error = PolicyProfile::parse("cinema/1").expect_err("cinema/1 must not parse");
        assert!(error.to_string().contains("streaming/1"));
    }

    /// TP-3.5: the row the record measures is the row a configured `bulk/1` resolves to, and the
    /// paths that must not change with it hold — no profile (today's numbers exactly), a switch
    /// that is off (the profile cannot enable the pack on its own) and an unknown id (refused
    /// naming the accepted values, never silently mapped to Balanced).
    #[cfg(feature = "policy-pack")]
    #[test]
    fn bulk_profile_path_and_its_fallbacks_are_unchanged() {
        use crate::policy_relay::PolicyRelay;

        let resolved = PolicyRelay::resolve("sched", Some(PolicyProfile::Bulk))
            .expect("a configured bulk/1 runs the pack");
        assert_eq!(resolved.stream_window, 2 * 1024 * 1024);
        assert_eq!(
            resolved.scheduler,
            Some(crate::policy_sched::SchedulerConfig {
                quantum_bytes: 64 * 1024,
                max_queued_bytes: 256 * 1024,
            })
        );
        assert_eq!(
            Mechanism::for_profile(PolicyProfile::Bulk).label(),
            "sched-w2097152-q65536-cap262144",
            "the evidence arm must be the row the config resolves"
        );
        // Absent profile: the switch's own numbers, exactly as before TP-3.1.
        assert_eq!(
            PolicyRelay::resolve("sched", None),
            PolicyRelay::parse("sched")
        );
        assert_eq!(PolicyRelay::resolve("", None), None);
        // A configured profile selects a row; it never turns the pack on by itself.
        assert_eq!(PolicyRelay::resolve("off", Some(PolicyProfile::Bulk)), None);
        let error = PolicyProfile::parse("download/1").expect_err("download/1 must not parse");
        assert!(error.to_string().contains("bulk/1"));
    }

    #[test]
    fn window_arms_sit_inside_the_product_window() {
        for mechanism in mechanism_matrix() {
            match mechanism {
                // The smallest budget Yamux can credit is half the receiver's window.
                Mechanism::CreditBudget { budget } => {
                    assert_eq!(budget, PRODUCT_WINDOW as usize / 2)
                }
                // The window arm must stay above Yamux's initial window (256 KiB) and below the
                // product value.
                Mechanism::WindowCap { bytes } => {
                    assert!((256 * 1024..PRODUCT_WINDOW).contains(&bytes))
                }
                _ => {}
            }
        }
    }

    #[tokio::test]
    async fn smoke_collects_samples_and_labels_simulated() {
        let idle = run_scenario(
            "idle",
            false,
            RATE,
            Duration::from_millis(300),
            Duration::from_millis(20),
        )
        .await;
        assert!(idle.samples >= 5, "samples: {}", idle.samples);
        assert_eq!(idle.evidence, "simulated");
        assert_eq!(idle.mechanism, "baseline");
        assert!(idle.json(RATE).contains("\"evidence\":\"simulated\""));
    }

    #[tokio::test]
    async fn aged_profile_switches_rate_by_connection_age() {
        let profile = Profile {
            rate: 1_000_000,
            aged: Some((Duration::from_secs(2), 21_000)),
        };
        assert_eq!(profile.rate_at(Duration::from_secs(1)), 1_000_000);
        assert_eq!(profile.rate_at(Duration::from_secs(2)), 21_000);
        assert_eq!(Profile::fixed(5).rate_at(Duration::from_secs(99)), 5);
    }

    /// Pushes `bytes` through `credit_relay` with `budget`, then reports how many bytes reached the
    /// sink and whether the sink saw end-of-stream. The writer runs in its own task so a flow that
    /// stops making progress is measured instead of hanging the test.
    async fn credit_transfer(budget: usize, bytes: usize, settle: Duration) -> (usize, bool) {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        };

        let (client_io, server_io) = shaped_endpoints(Profile::fixed(8_000_000)).await;
        let received = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicBool::new(false));
        let sink_received = Arc::clone(&received);
        let sink_finished = Arc::clone(&finished);
        let mut server_session = Session::new_server(server_io, yamux_config(8));
        let server_task = tokio::spawn(async move {
            while let Some(item) = server_session.next().await {
                let Ok(stream) = item else { break };
                let sink_received = Arc::clone(&sink_received);
                let sink_finished = Arc::clone(&sink_finished);
                tokio::spawn(async move {
                    let (local, mut app) = duplex(64 * 1024);
                    tokio::spawn(async move {
                        let _ = relay_with_idle_timeout(stream, local, IDLE).await;
                    });
                    let mut buffer = vec![0_u8; PUMP_READ];
                    loop {
                        match app.read(&mut buffer).await {
                            Ok(0) => {
                                sink_finished.store(true, Ordering::SeqCst);
                                return;
                            }
                            Ok(count) => {
                                sink_received.fetch_add(count, Ordering::SeqCst);
                            }
                            Err(_) => return,
                        }
                    }
                });
            }
        });
        let mut client_session = Session::new_client(client_io, yamux_config(8));
        let mut control = client_session.control();
        let client_task = tokio::spawn(async move {
            while let Some(item) = client_session.next().await {
                if item.is_err() {
                    break;
                }
            }
        });
        let stream = control.open_stream().await.expect("open stream");
        let (local, mut app) = duplex(64 * 1024);
        tokio::spawn(async move {
            let _ = credit_relay(stream, local, budget, PRODUCT_WINDOW).await;
        });
        tokio::spawn(async move {
            let payload = vec![0x5a_u8; bytes];
            if app.write_all(&payload).await.is_ok() {
                let _ = app.shutdown().await;
            }
        });
        let watch = Arc::clone(&finished);
        let done = tokio::time::timeout(settle, async move {
            while !watch.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok();
        let total = received.load(Ordering::SeqCst);
        server_task.abort();
        client_task.abort();
        (total, done)
    }

    /// M1's structural limit (ADR-0048 decision 4): Yamux returns credit only once the receiver has
    /// consumed half of its window (2 MiB of the product's 4 MiB), so a relay budget below that
    /// never receives a window update and the flow stops at the budget. This is why the chosen
    /// bound is applied at the window instead of as a relay byte counter.
    #[tokio::test]
    async fn credit_budget_below_the_window_granularity_stalls() {
        let (total, done) = credit_transfer(64 * 1024, 256 * 1024, Duration::from_secs(1)).await;
        assert!(!done, "a 64 KiB credit budget must not finish the transfer");
        assert!(
            (32 * 1024..=64 * 1024).contains(&total),
            "the flow must stop at the budget, got {total}"
        );
    }

    /// The smallest budget Yamux can credit (half the window) does move every byte, but the bound
    /// it enforces is 2 MiB, still about 0.8 s in front of a 2.5 MB/s bottleneck.
    #[tokio::test]
    async fn credit_budget_at_half_the_window_completes() {
        let bytes = 4 * 1024 * 1024;
        let (total, done) =
            credit_transfer(PRODUCT_WINDOW as usize / 2, bytes, Duration::from_secs(20)).await;
        assert!(
            done,
            "the transfer must finish, got {total} of {bytes} bytes"
        );
        assert_eq!(total, bytes);
    }

    /// Aged-session throttle (scaled): a fresh connection moves 1 MiB quickly, the same
    /// transfer on a connection past the threshold is throttled. Prints JSON lines.
    #[tokio::test]
    #[ignore = "measurement run; use --release --ignored --nocapture"]
    async fn aged_session_throttled_transfer() {
        let profile = Profile {
            rate: 2_500_000,
            aged: Some((Duration::from_secs(3), 200_000)),
        };
        let bytes = 1024 * 1024;
        let fresh = run_transfer(profile, Duration::ZERO, bytes).await;
        let aged = run_transfer(profile, Duration::from_secs(4), bytes).await;
        println!(
            "POLICY_BASELINE {{\"scenario\":\"aged_session_throttled\",\"mechanism\":\"baseline\",\"evidence\":\"simulated\",\"bytes\":{bytes},\"fresh_ms\":{fresh},\"aged_ms\":{aged},\"rate_bytes_per_s\":2500000,\"aged_rate_bytes_per_s\":200000,\"aged_after_s\":3}}"
        );
    }

    /// The same aged-session scenario under the chosen bound (the Yamux window, 256 KiB): the
    /// bound must not hide or worsen the throttle (ADR-0048 experiments).
    #[tokio::test]
    #[ignore = "measurement run; use --release --ignored --nocapture"]
    async fn aged_session_under_window_cap() {
        let profile = Profile {
            rate: 2_500_000,
            aged: Some((Duration::from_secs(3), 200_000)),
        };
        let bytes = 1024 * 1024;
        let mechanism = Mechanism::WindowCap { bytes: 256 * 1024 };
        let fresh = run_transfer_mechanism(profile, Duration::ZERO, bytes, mechanism).await;
        let aged = run_transfer_mechanism(profile, Duration::from_secs(4), bytes, mechanism).await;
        println!(
            "POLICY_BASELINE {{\"scenario\":\"aged_session_throttled\",\"mechanism\":\"{}\",\"evidence\":\"simulated\",\"bytes\":{bytes},\"fresh_ms\":{fresh},\"aged_ms\":{aged},\"rate_bytes_per_s\":2500000,\"aged_rate_bytes_per_s\":200000,\"aged_after_s\":3}}",
            mechanism.label()
        );
    }

    /// The aged-session throttle under the shipped `sched` switch: arbitration must leave the
    /// throttle exactly as it is on the window arm (ADR-0048 decision 5).
    #[cfg(feature = "policy-pack")]
    #[tokio::test]
    #[ignore = "measurement run; use --release --ignored --nocapture"]
    async fn aged_session_under_scheduler() {
        let profile = Profile {
            rate: 2_500_000,
            aged: Some((Duration::from_secs(3), 200_000)),
        };
        let bytes = 1024 * 1024;
        let mechanism = Mechanism::Scheduler {
            window: 256 * 1024,
            quantum: 16 * 1024,
            cap: 64 * 1024,
        };
        let fresh = run_transfer_mechanism(profile, Duration::ZERO, bytes, mechanism).await;
        let aged = run_transfer_mechanism(profile, Duration::from_secs(4), bytes, mechanism).await;
        println!(
            "POLICY_BASELINE {{\"scenario\":\"aged_session_throttled\",\"mechanism\":\"{}\",\"evidence\":\"simulated\",\"bytes\":{bytes},\"fresh_ms\":{fresh},\"aged_ms\":{aged},\"rate_bytes_per_s\":2500000,\"aged_rate_bytes_per_s\":200000,\"aged_after_s\":3}}",
            mechanism.label()
        );
    }

    /// Experiment (ADR-0048, window cap): loaded ping RTT versus the Yamux per-stream window.
    /// The minimum is the initial window (256 KiB); the product value is 4 MiB.
    #[tokio::test]
    #[ignore = "measurement run; use --release --ignored --nocapture"]
    async fn window_cap_experiment() {
        for window in [256 * 1024_u32, 1024 * 1024, 4 * 1024 * 1024] {
            let mut summary = run_scenario_window(
                "bulk_competing_window",
                true,
                RATE,
                Duration::from_secs(8),
                Duration::from_millis(20),
                Some(window),
            )
            .await;
            summary.scenario = "bulk_competing_window";
            println!(
                "POLICY_EXPERIMENT window_bytes={window} {}",
                summary.json(RATE)
            );
        }
    }

    /// ADR-0048 mechanism comparison: baseline, M1/M2/M3 and the window arm at one bottleneck rate,
    /// repeated. Prints one JSON line per arm and repeat, all labelled `simulated`.
    #[tokio::test]
    #[ignore = "measurement run; use --release --ignored --nocapture"]
    async fn mechanism_experiments() {
        let (rate, repeats, seconds) = experiment_settings(8);
        let duration = Duration::from_secs(seconds);
        let interval = Duration::from_millis(20);

        let idle = run_scenario("idle", false, rate, duration, interval).await;
        println!("POLICY_EXPERIMENT {}", idle.json(rate));
        for mechanism in mechanism_matrix() {
            for repeat in 1..=repeats {
                let summary = run_scenario_mechanism(
                    "bulk_competing",
                    true,
                    rate,
                    duration,
                    interval,
                    mechanism,
                )
                .await;
                println!(
                    "POLICY_EXPERIMENT {{\"repeat\":{repeat},\"mechanism\":\"{}\",\"bulk_bytes_per_s\":{}}} {}",
                    summary.mechanism,
                    summary.bulk_bytes / seconds.max(1),
                    summary.json(rate)
                );
            }
        }
    }

    /// The CPU accounting the overhead budget (ADR-0048 decision 6) rests on: it must be a real
    /// clock, not a constant, or the per-MB figures would be meaningless.
    #[test]
    fn process_cpu_time_advances_while_work_runs() {
        let start = process_cpu_us();
        let until = Instant::now() + Duration::from_millis(30);
        let mut acc = 0_u64;
        while Instant::now() < until {
            acc = acc.wrapping_mul(6364136223846793005).wrapping_add(1);
        }
        assert_ne!(acc, 0, "the busy loop must run");
        let elapsed = process_cpu_us().saturating_sub(start);
        assert!(elapsed > 0, "process CPU must advance, got {elapsed} us");
    }

    /// The probe behind the per-MB CPU figures: one unshaped loopback stream, both ends in-process,
    /// every byte must arrive so the CPU window covered the same relayed work in every arm.
    #[tokio::test]
    async fn relay_cpu_probe_relays_every_byte() {
        let bytes = 8 * 1024 * 1024;
        let (cpu_us, delivered) =
            relay_cpu_probe(Mechanism::WindowCap { bytes: 256 * 1024 }, bytes).await;
        assert_eq!(delivered, bytes, "the probe must relay every byte");
        assert!(cpu_us > 0, "the probe must see CPU time");
    }

    /// ADR-0048 decision 6, first half, clean figure: CPU per MB relayed on an unshaped loopback
    /// (no shaper work in the denominator), three arms, `POLICY_REPEATS` times. One JSON line per
    /// repeat, `simulated`.
    #[cfg(feature = "policy-pack")]
    #[tokio::test]
    #[ignore = "measurement run; use --release --ignored --nocapture"]
    async fn relay_cpu_probe_experiments() {
        let (_, repeats, _) = experiment_settings(6);
        let bytes: u64 = std::env::var("POLICY_BYTES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(128 * 1024 * 1024);
        let arms = [
            Mechanism::Baseline,
            Mechanism::WindowCap { bytes: 256 * 1024 },
            Mechanism::Scheduler {
                window: 256 * 1024,
                quantum: 16 * 1024,
                cap: 64 * 1024,
            },
        ];
        for mechanism in arms {
            for repeat in 1..=repeats {
                let (cpu_us, delivered) = relay_cpu_probe(mechanism, bytes).await;
                println!(
                    "POLICY_EXPERIMENT {{\"scenario\":\"relay_cpu_probe\",\"mechanism\":\"{}\",\"evidence\":\"simulated\",\"repeat\":{},\"bytes\":{},\"delivered_bytes\":{},\"cpu_us\":{},\"cpu_us_per_mb\":{:.1}}}",
                    mechanism.label(),
                    repeat,
                    bytes,
                    delivered,
                    cpu_us,
                    cpu_us as f64 / (delivered as f64 / (1024.0 * 1024.0)).max(f64::MIN_POSITIVE)
                );
            }
        }
    }

    /// One profile's evidence run: Baseline, `balanced/1` and `profile` under bulk competition,
    /// then two competing flows with a mid-run close, then the per-MB CPU probe, plus an idle
    /// measurement when the record needs one. The arms come from `Mechanism::for_profile`, i.e. the
    /// shipped table, and every line is labelled `simulated`. This is not a LAB run and does not
    /// settle a profile's acceptance (ADR-0031 decision 19).
    #[cfg(feature = "policy-pack")]
    async fn run_profile_evidence(
        run_id: &str,
        profile: PolicyProfile,
        bulk_scenario: &'static str,
        multi_scenario: &'static str,
        idle_scenario: Option<&'static str>,
    ) {
        let (rate, repeats, seconds) = experiment_settings(8);
        let duration = Duration::from_secs(seconds);
        let interval = Duration::from_millis(20);
        let arms = [
            Mechanism::Baseline,
            Mechanism::for_profile(PolicyProfile::Balanced),
            Mechanism::for_profile(profile),
        ];

        if let Some(scenario) = idle_scenario {
            for mechanism in arms {
                let summary =
                    run_scenario_mechanism(scenario, false, rate, duration, interval, mechanism)
                        .await;
                println!(
                    "POLICY_EXPERIMENT {{\"profile_run\":\"{run_id}\",\"repeat\":1}} {}",
                    summary.json(rate)
                );
            }
        }

        for mechanism in arms {
            for repeat in 1..=repeats {
                let summary = run_scenario_mechanism(
                    bulk_scenario,
                    true,
                    rate,
                    duration,
                    interval,
                    mechanism,
                )
                .await;
                println!(
                    "POLICY_EXPERIMENT {{\"profile_run\":\"{run_id}\",\"repeat\":{repeat},\"bulk_bytes_per_s\":{}}} {}",
                    summary.bulk_bytes / seconds.max(1),
                    summary.json(rate)
                );
            }
        }

        // Two competing flows and three competing flows, each closing the first flow mid-run. With
        // two flows one survives, so its split is trivial; the three-flow run is the one that shows
        // whether two survivors share the bottleneck evenly (TP-2.2's measurement).
        for flows in [2, 3] {
            for mechanism in arms {
                let summary = run_multi_flow(
                    multi_scenario,
                    flows,
                    true,
                    rate,
                    duration,
                    interval,
                    mechanism,
                )
                .await;
                println!(
                    "POLICY_EXPERIMENT {{\"profile_run\":\"{run_id}\",\"repeat\":1,\"flows\":{flows}}} {}",
                    summary.json()
                );
            }
        }

        let bytes: u64 = std::env::var("POLICY_BYTES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(128 * 1024 * 1024);
        for mechanism in arms {
            for repeat in 1..=repeats {
                let (cpu_us, delivered) = relay_cpu_probe(mechanism, bytes).await;
                println!(
                    "POLICY_EXPERIMENT {{\"profile_run\":\"{run_id}\",\"scenario\":\"relay_cpu_probe\",\"mechanism\":\"{}\",\"evidence\":\"simulated\",\"repeat\":{},\"bytes\":{},\"delivered_bytes\":{},\"cpu_us\":{},\"cpu_us_per_mb\":{:.1}}}",
                    mechanism.label(),
                    repeat,
                    bytes,
                    delivered,
                    cpu_us,
                    cpu_us as f64 / (delivered as f64 / (1024.0 * 1024.0)).max(f64::MIN_POSITIVE)
                );
            }
        }
    }

    /// TP-3.2 Gaming evidence run: the harness-side record for
    /// `docs/POLICY_PACK_TP-3.2_GAMING_EVIDENCE.md`. No idle arm: that record shipped without one.
    #[cfg(feature = "policy-pack")]
    #[tokio::test]
    #[ignore = "measurement run; use --release --features policy-pack --ignored --nocapture"]
    async fn gaming_profile_evidence() {
        run_profile_evidence(
            "tp-3.2",
            PolicyProfile::Gaming,
            "gaming_bulk_competing",
            "gaming_multi_flow",
            None,
        )
        .await;
    }

    /// TP-3.3 Trading evidence run: the harness-side record for
    /// `docs/POLICY_PACK_TP-3.3_TRADING_EVIDENCE.md`. It adds the idle arm because criterion 1
    /// names idle latency and the harness can supply it per arm.
    #[cfg(feature = "policy-pack")]
    #[tokio::test]
    #[ignore = "measurement run; use --release --features policy-pack --ignored --nocapture"]
    async fn trading_profile_evidence() {
        run_profile_evidence(
            "tp-3.3",
            PolicyProfile::Trading,
            "trading_bulk_competing",
            "trading_multi_flow",
            Some("trading_idle"),
        )
        .await;
    }

    /// TP-3.4 Streaming evidence run: the harness-side record for
    /// `docs/POLICY_PACK_TP-3.4_STREAMING_EVIDENCE.md`. Like TP-3.3 it includes the idle arm,
    /// because criterion 1 names idle latency and the harness can supply it per arm.
    #[cfg(feature = "policy-pack")]
    #[tokio::test]
    #[ignore = "measurement run; use --release --features policy-pack --ignored --nocapture"]
    async fn streaming_profile_evidence() {
        run_profile_evidence(
            "tp-3.4",
            PolicyProfile::Streaming,
            "streaming_bulk_competing",
            "streaming_multi_flow",
            Some("streaming_idle"),
        )
        .await;
    }

    /// TP-3.5 Bulk evidence run: the harness-side record for
    /// `docs/POLICY_PACK_TP-3.5_BULK_EVIDENCE.md`. Like TP-3.3 and TP-3.4 it includes the idle arm,
    /// because criterion 1 names idle latency and the harness can supply it per arm.
    #[cfg(feature = "policy-pack")]
    #[tokio::test]
    #[ignore = "measurement run; use --release --features policy-pack --ignored --nocapture"]
    async fn bulk_profile_evidence() {
        run_profile_evidence(
            "tp-3.5",
            PolicyProfile::Bulk,
            "bulk_bulk_competing",
            "bulk_multi_flow",
            Some("bulk_idle"),
        )
        .await;
    }

    /// The multi-flow harness itself, on an arm that exists without the feature: every flow must
    /// move bytes, the split must be visible in the per-flow counters, and per-flow latency must
    /// be reported for each flow.
    #[tokio::test]
    async fn multi_flow_scenario_moves_every_flow() {
        let summary = run_multi_flow(
            "multi_flow_smoke",
            2,
            false,
            2_000_000,
            Duration::from_millis(600),
            Duration::from_millis(20),
            Mechanism::WindowCap { bytes: 256 * 1024 },
        )
        .await;
        assert!(!summary.stalled, "the multi-flow run must not stall");
        assert_eq!(summary.flows, 2);
        assert_eq!(summary.bulk_bytes.len(), 2);
        assert!(summary.bulk_bytes.iter().all(|bytes| *bytes > 0));
        assert!(
            summary.delivery_min_over_max() > 0.5,
            "an unarbitrated pair still shares the bottleneck, got {}",
            summary.delivery_min_over_max()
        );
        assert_eq!(summary.flow_p50_us.len(), 2);
        assert_eq!(summary.flow_p95_us.len(), 2);
        assert!(summary.json().contains("\"evidence\":\"simulated\""));
    }

    /// A flow that closes mid-run must not stop the others and must not wedge the relay: the
    /// surviving flows keep handing bytes after the close, and the run finishes inside its deadline.
    #[tokio::test]
    async fn multi_flow_churn_keeps_surviving_flows_moving() {
        let summary = run_multi_flow(
            "multi_flow_churn_smoke",
            2,
            true,
            2_000_000,
            Duration::from_millis(900),
            Duration::from_millis(20),
            Mechanism::WindowCap { bytes: 256 * 1024 },
        )
        .await;
        assert!(!summary.stalled, "churn must not stall the run");
        assert_eq!(summary.churn_closed, Some(0));
        assert!(
            summary.continued_after_churn,
            "the surviving flow must keep moving after the other closes: {:?}",
            summary.bulk_bytes
        );
        assert!(summary.bulk_bytes[1] > 0);
    }

    /// The scheduler's claim is arbitration between competing flows. Three bulk flows must all get
    /// a share of the same bottleneck, each with its own latency reported, and a churned flow must
    /// leave the rotation instead of stalling the survivors.
    #[cfg(feature = "policy-pack")]
    #[tokio::test]
    async fn scheduler_arbitrates_between_multi_flows_evenly() {
        let mechanism = Mechanism::Scheduler {
            window: 256 * 1024,
            quantum: 16 * 1024,
            cap: 64 * 1024,
        };
        let summary = run_multi_flow(
            "multi_flow_smoke",
            3,
            false,
            2_000_000,
            Duration::from_millis(800),
            Duration::from_millis(20),
            mechanism,
        )
        .await;
        assert!(!summary.stalled, "the scheduler must not stall the flows");
        assert!(
            summary.bulk_bytes.iter().all(|bytes| *bytes > 0),
            "every flow must be served: {:?}",
            summary.bulk_bytes
        );
        assert!(
            summary.delivery_min_over_max() > 0.5,
            "three equal flows must split the bottleneck, smallest/largest = {}",
            summary.delivery_min_over_max()
        );
        assert!(
            summary.flow_p95_us.iter().all(|value| *value > 0),
            "every flow must report a latency: {:?}",
            summary.flow_p95_us
        );

        let churned = run_multi_flow(
            "multi_flow_churn_smoke",
            2,
            true,
            2_000_000,
            Duration::from_millis(900),
            Duration::from_millis(20),
            mechanism,
        )
        .await;
        assert!(!churned.stalled, "churn must not stall the arbitration");
        assert!(churned.continued_after_churn);
        assert!(churned.bulk_bytes[1] > 0);
    }

    /// ADR-0048 decision 6, first half: the CPU cost per MB the arm hands to the engine, against
    /// Baseline and the window pump, at the matrix rates and repeats. Also runs each arm idle to
    /// check the decision's "no more than 1 ms median added latency on an idle tunnel". One JSON
    /// line per arm and repeat, all `simulated`.
    #[cfg(feature = "policy-pack")]
    #[tokio::test]
    #[ignore = "measurement run; use --release --ignored --nocapture"]
    async fn cpu_cost_experiments() {
        let (rate, repeats, seconds) = experiment_settings(6);
        let duration = Duration::from_secs(seconds);
        let interval = Duration::from_millis(20);
        let arms = [
            Mechanism::Baseline,
            Mechanism::WindowCap { bytes: 256 * 1024 },
            Mechanism::Scheduler {
                window: 256 * 1024,
                quantum: 16 * 1024,
                cap: 64 * 1024,
            },
        ];
        for mechanism in arms {
            let idle =
                run_scenario_mechanism("idle", false, rate, duration, interval, mechanism).await;
            println!("POLICY_EXPERIMENT idle {}", idle.json(rate));
        }
        for mechanism in arms {
            for repeat in 1..=repeats {
                let summary = run_scenario_mechanism(
                    "bulk_competing_cpu",
                    true,
                    rate,
                    duration,
                    interval,
                    mechanism,
                )
                .await;
                let handed_mb = summary.bulk_bytes as f64 / (1024.0 * 1024.0);
                let delivered_mb = (rate * seconds) as f64 / (1024.0 * 1024.0);
                println!(
                    "POLICY_EXPERIMENT {{\"repeat\":{repeat},\"mechanism\":\"{}\",\"cpu_us_per_mb_handed\":{:.1},\"cpu_us_per_mb_delivered\":{:.1}}} {}",
                    summary.mechanism,
                    summary.cpu_us as f64 / handed_mb.max(f64::MIN_POSITIVE),
                    summary.cpu_us as f64 / delivered_mb.max(f64::MIN_POSITIVE),
                    summary.json(rate)
                );
            }
        }
    }

    /// TP-2.2 multi-flow arm: the scheduler's actual claim (arbitration between competing flows)
    /// measured against the window pump and Baseline at the matrix rates and repeats. Two flow
    /// counts, plus a churn variant that closes one flow at half the run, all `simulated`.
    #[cfg(feature = "policy-pack")]
    #[tokio::test]
    #[ignore = "measurement run; use --release --ignored --nocapture"]
    async fn multi_flow_experiments() {
        let (rate, repeats, seconds) = experiment_settings(6);
        let duration = Duration::from_secs(seconds);
        let interval = Duration::from_millis(20);
        let arms = [
            Mechanism::Baseline,
            Mechanism::WindowCap { bytes: 256 * 1024 },
            Mechanism::Scheduler {
                window: 256 * 1024,
                quantum: 16 * 1024,
                cap: 64 * 1024,
            },
        ];
        for mechanism in arms {
            for flows in [2_usize, 3] {
                for repeat in 1..=repeats {
                    let summary = run_multi_flow(
                        "multi_flow",
                        flows,
                        false,
                        rate,
                        duration,
                        interval,
                        mechanism,
                    )
                    .await;
                    println!(
                        "POLICY_EXPERIMENT {{\"repeat\":{repeat},\"rate_bytes_per_s\":{rate}}} {}",
                        summary.json()
                    );
                }
            }
        }
        // Churn: flow 0 closes at half the run; the survivors must keep moving.
        for mechanism in arms {
            for repeat in 1..=repeats {
                let summary = run_multi_flow(
                    "multi_flow_churn",
                    2,
                    true,
                    rate,
                    duration,
                    interval,
                    mechanism,
                )
                .await;
                println!(
                    "POLICY_EXPERIMENT {{\"repeat\":{repeat},\"rate_bytes_per_s\":{rate}}} {}",
                    summary.json()
                );
            }
        }
    }

    /// ADR-0056 decision 12: the adaptive loop against the static row it starts from, on the same
    /// shaped bottleneck. Two arms at three rates, plus one long run so a down-step can reach its
    /// verification window and be kept. Every line is `simulated`. Reads `POLICY_ADAPTIVE_SECONDS`
    /// (short runs, default 8) and `POLICY_ADAPTIVE_LONG_SECONDS` (long run, default 150).
    #[cfg(feature = "policy-pack")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "measurement run; use --release --features policy-pack --ignored --nocapture"]
    async fn adaptive_controller_experiments() {
        use crate::policy_profile::PolicyProfile;

        let seconds: u64 = std::env::var("POLICY_ADAPTIVE_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(8);
        let long_seconds: u64 = std::env::var("POLICY_ADAPTIVE_LONG_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(150);
        let interval = Duration::from_millis(20);
        // Start at the pack's default row: its 256 KiB window is small enough that the shaped
        // bottleneck's backpressure reaches the arbiter, which is where the loop takes its sample.
        let profile = PolicyProfile::Balanced;
        let arms = [
            Mechanism::for_profile(profile),
            Mechanism::adaptive_from(profile),
        ];
        for rate in [250_000_u64, 500_000, 1_000_000] {
            for mechanism in arms {
                for repeat in 1..=2 {
                    let summary = run_multi_flow(
                        "adaptive_control",
                        3,
                        false,
                        rate,
                        Duration::from_secs(seconds),
                        interval,
                        mechanism,
                    )
                    .await;
                    println!(
                        "POLICY_ADAPTIVE_EXPERIMENT {{\"repeat\":{repeat},\"rate_bytes_per_s\":{rate},\"seconds\":{seconds}}} {}",
                        summary.json()
                    );
                }
            }
        }
        // One long run at the slowest rate: 3 s of queueing starts a down-step, the 60 s
        // verification window decides it, and the 30 s dwell plus the 300 s rate limit gate the
        // next one, so this is where changes and their spacing are actually observable.
        let summary = run_multi_flow(
            "adaptive_control_long",
            3,
            false,
            250_000,
            Duration::from_secs(long_seconds),
            interval,
            Mechanism::adaptive_from(profile),
        )
        .await;
        println!(
            "POLICY_ADAPTIVE_EXPERIMENT {{\"repeat\":0,\"rate_bytes_per_s\":250000,\"seconds\":{long_seconds}}} {}",
            summary.json()
        );
    }

    /// Full Baseline scenarios; prints one JSON line per scenario. Not part of the normal run.
    #[tokio::test]
    #[ignore = "measurement run; use --release --ignored --nocapture"]
    async fn baseline_idle_and_loaded() {
        for (name, bulk) in [("idle", false), ("bulk_competing", true)] {
            let s = run_scenario(
                name,
                bulk,
                RATE,
                Duration::from_secs(10),
                Duration::from_millis(20),
            )
            .await;
            println!("POLICY_BASELINE {}", s.json(RATE));
        }
    }
}
