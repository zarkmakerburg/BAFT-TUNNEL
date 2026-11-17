//! Policy Pack measurement harness, TP-1.2 first stage (ADR-0031, docs/POLICY_PACK_TP-0.1_RESEARCH.md).
//!
//! Test-only. Measures the round-trip time of a small interactive stream with and without a
//! competing bulk stream, through the product's own `yamux_config` and `relay_with_idle_timeout`
//! (the real relay path, both ends) over loopback TCP joined by a rate-limited proxy. Evidence
//! level of every number is `simulated` (never VM or LAB). It measures today's data path
//! (Baseline, pack off); the policy engine does not exist yet.
//!
//! Run: `cargo test --release policy_baseline -- --ignored --nocapture`.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use futures::StreamExt;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex},
    net::{TcpListener, TcpStream},
};
use tokio_yamux::Session;

use crate::tunnel::{relay_with_idle_timeout, yamux_config};

const PING: usize = 64;
const PUMP_READ: usize = 16 * 1024;
const IDLE: Duration = Duration::from_secs(900);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Summary {
    pub scenario: &'static str,
    pub evidence: &'static str,
    pub samples: usize,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
    pub bulk_bytes: u64,
}

impl Summary {
    pub(crate) fn json(&self, rate_bytes_per_s: u64) -> String {
        format!(
            "{{\"scenario\":\"{}\",\"evidence\":\"{}\",\"rate_bytes_per_s\":{},\"samples\":{},\"rtt_us\":{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}},\"bulk_bytes\":{}}}",
            self.scenario,
            self.evidence,
            rate_bytes_per_s,
            self.samples,
            self.p50_us,
            self.p95_us,
            self.p99_us,
            self.max_us,
            self.bulk_bytes
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

/// Two loopback TCP endpoints joined by a bottleneck proxy of `rate` bytes/s each way.
async fn shaped_endpoints(profile: Profile) -> (TcpStream, TcpStream) {
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
    (client, server)
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
    let (client_io, server_io) = shaped_endpoints(Profile::fixed(rate)).await;
    let mut server_session = Session::new_server(server_io, yamux_config(8));
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
    let mut client_session = Session::new_client(client_io, yamux_config(8));
    let control = client_session.control();
    let client_task = tokio::spawn(async move {
        while let Some(item) = client_session.next().await {
            if item.is_err() {
                break;
            }
        }
    });

    let open = |mut control: tokio_yamux::Control| async move {
        let stream = control.open_stream().await.expect("open stream");
        let (local, app): (DuplexStream, DuplexStream) = duplex(64 * 1024);
        tokio::spawn(async move {
            let _ = relay_with_idle_timeout(stream, local, IDLE).await;
        });
        app
    };

    let bulk_sent = Arc::new(AtomicU64::new(0));
    let bulk_task = if with_bulk {
        let mut app = open(control.clone()).await;
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

    let mut interactive = open(control.clone()).await;
    let ping = [7_u8; PING];
    let mut echo = [0_u8; PING];
    let mut samples = Vec::new();
    let end = Instant::now() + duration;
    while Instant::now() < end {
        let start = Instant::now();
        interactive.write_all(&ping).await.expect("ping");
        interactive.read_exact(&mut echo).await.expect("echo");
        samples.push(start.elapsed().as_micros() as u64);
        tokio::time::sleep(interval).await;
    }

    if let Some(task) = bulk_task {
        task.abort();
    }
    let bulk_bytes = bulk_sent.load(Ordering::Relaxed);
    server_task.abort();
    client_task.abort();
    samples.sort_unstable();
    Summary {
        scenario,
        evidence: "simulated",
        samples: samples.len(),
        p50_us: percentile(&samples, 50.0),
        p95_us: percentile(&samples, 95.0),
        p99_us: percentile(&samples, 99.0),
        max_us: samples.last().copied().unwrap_or(0),
        bulk_bytes,
    }
}

/// Time to push `bytes` through one stream (acknowledged by the receiver with one byte) after the
/// connection has existed for `wait`. Models the aged-session throttle: compare `wait` below and
/// above the profile's threshold. Returns elapsed milliseconds.
pub(crate) async fn run_transfer(profile: Profile, wait: Duration, bytes: usize) -> u64 {
    let (client_io, server_io) = shaped_endpoints(profile).await;
    let mut server_session = Session::new_server(server_io, yamux_config(8));
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
    let mut client_session = Session::new_client(client_io, yamux_config(8));
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
    tokio::spawn(async move {
        let _ = relay_with_idle_timeout(stream, local, IDLE).await;
    });
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
            "POLICY_BASELINE {{\"scenario\":\"aged_session_throttled\",\"evidence\":\"simulated\",\"bytes\":{bytes},\"fresh_ms\":{fresh},\"aged_ms\":{aged},\"rate_bytes_per_s\":2500000,\"aged_rate_bytes_per_s\":200000,\"aged_after_s\":3}}"
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
