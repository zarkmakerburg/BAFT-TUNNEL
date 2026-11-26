//! Policy Pack measurement harness, TP-1.2 first stage (ADR-0031, docs/POLICY_PACK_TP-0.1_RESEARCH.md).
//!
//! Test-only. Measures the round-trip time of a small interactive stream with and without a
//! competing bulk stream, through the product's own `yamux_config` and `relay_with_idle_timeout`
//! (the real relay path, both ends) over loopback TCP joined by a rate-limited proxy. Evidence
//! level of every number is `simulated` (never VM or LAB). It measures today's data path
//! (Baseline, pack off) plus the ADR-0048 mechanism variants M1/M2/M3 behind test-only switches.
//!
//! Run: `cargo test --release policy_baseline -- --ignored --nocapture`.
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

use crate::tunnel::{relay_with_idle_timeout, yamux_config};

const PING: usize = 64;
const PUMP_READ: usize = 16 * 1024;
const IDLE: Duration = Duration::from_secs(900);
/// Product Yamux per-stream window (`tunnel::MAX_STREAM_WINDOW_SIZE`), the in-flight ceiling.
const PRODUCT_WINDOW: u32 = 4 * 1024 * 1024;

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
    /// True when the interactive ping did not return inside the arm's deadline. An arm that
    /// stalls its interactive flow is reported, not hidden: the harness never hangs the suite.
    pub stalled: bool,
}

impl Summary {
    pub(crate) fn json(&self, rate_bytes_per_s: u64) -> String {
        format!(
            "{{\"scenario\":\"{}\",\"mechanism\":\"{}\",\"evidence\":\"{}\",\"rate_bytes_per_s\":{},\"samples\":{},\"rtt_us\":{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}},\"bulk_bytes\":{},\"stalled\":{}}}",
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
        }
    }
}

/// The Yamux per-stream window an arm runs with: the arm's own cap, or the product value.
fn window_for(mechanism: Mechanism) -> Option<u32> {
    match mechanism {
        Mechanism::WindowCap { bytes } => Some(bytes),
        _ => None,
    }
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
    let window = window.or(window_for(mechanism));
    let mut server_session = Session::new_server(server_io, config_with_window(window));
    let sink_mechanism = mechanism;
    let server_task = tokio::spawn(async move {
        let mut index = 0_u32;
        while let Some(item) = server_session.next().await {
            let Ok(stream) = item else { break };
            let is_sink = with_bulk && index == 0;
            index += 1;
            tokio::spawn(async move {
                let (local, app) = duplex(64 * 1024);
                tokio::spawn(async move {
                    let _ = relay_with_idle_timeout(stream, local, IDLE).await;
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

    let open = |mut control: tokio_yamux::Control, mechanism: Mechanism| async move {
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
                    let _ = relay_with_idle_timeout(stream, local, IDLE).await;
                });
            }
        }
        app
    };

    let bulk_sent = Arc::new(AtomicU64::new(0));
    let bulk_task = if with_bulk {
        let mut app = open(control.clone(), mechanism).await;
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

    let mut interactive = open(control.clone(), Mechanism::Baseline).await;
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
    let server_task = tokio::spawn(async move {
        while let Some(item) = server_session.next().await {
            let Ok(stream) = item else { break };
            tokio::spawn(async move {
                let (local, app) = duplex(64 * 1024);
                tokio::spawn(async move {
                    let _ = relay_with_idle_timeout(stream, local, IDLE).await;
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
    match mechanism {
        Mechanism::CreditBudget { budget } => {
            tokio::spawn(async move {
                let _ = credit_relay(stream, local, budget, PRODUCT_WINDOW).await;
            });
        }
        _ => {
            tokio::spawn(async move {
                let _ = relay_with_idle_timeout(stream, local, IDLE).await;
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

/// The arms of the ADR-0048 comparison: today's relay, the three candidate bounds, and the window
/// arm. M1 runs at half the product window because that is the smallest budget Yamux can credit:
/// a receiver emits a window update only once `max_recv_window / 2` bytes are consumed (see
/// `credit_budget_below_the_window_granularity_stalls`), so a smaller budget stalls the flow.
pub(crate) fn mechanism_matrix() -> [Mechanism; 5] {
    [
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
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u64 = 2_500_000; // about 20 Mbit/s

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
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len());
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
        let rate: u64 = std::env::var("POLICY_RATE")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(RATE);
        let repeats: usize = std::env::var("POLICY_REPEATS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(2);
        let seconds: u64 = std::env::var("POLICY_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(8);
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
