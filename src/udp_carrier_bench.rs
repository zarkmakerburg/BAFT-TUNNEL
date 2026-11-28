//! Gate (d) of ADR-0046: a per-tuple benchmark of the prototype QUIC datagram carrier against the
//! product's TCP carrier layers, behind the non-default `udp-carrier` feature and compiled only for
//! tests. It changes no default build, no default test list and no product surface.
//!
//! What it actually measures, on loopback, in one process:
//!
//! - **UDP tuple:** a real QUIC session through `crate::udp_carrier` — handshake, the control-plane
//!   hello, negotiated datagram extension — then unreliable datagram request/echo round trips.
//! - **TCP tuple:** the product's TCP carrier layers — a TLS 1.3 session on `ring` with the pinned
//!   certificate, carrying a `tokio_yamux::Session` built with `crate::tunnel::yamux_config(8)`, then
//!   one Yamux stream with the same request/echo messages.
//!
//! The tuple axis is (carrier, application message size) because ADR-0046 decision 10 leaves the
//! mapping of the five policy profiles onto datagram classes undecided, so a profile axis here would
//! be invented. Two message sizes are used: 64 B (interactive) and 1024 B (large datagram); both fit
//! inside the 1200-byte path-MTU assumption.
//!
//! It **reports** p50/p95/p99 latency and achieved echo throughput per tuple and writes a stable
//! machine-readable artifact (`BAFT_UDP_BENCH_OUT`, default `target/udp-carrier-bench.json`). It
//! **enforces** only conservative sanity bounds, never absolute performance: every sample must be a
//! byte-exact echo, and the datagram carrier must stay within 10x of the TCP carrier's p50 latency
//! (or +5 ms, whichever is larger) and within a 10x throughput floor. Nothing here is a
//! performance claim: it is loopback, single-host, one process, one run.

use crate::tunnel::yamux_config;
use crate::udp_carrier as carrier;
use anyhow::{Result, ensure};
use futures::StreamExt;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use serde_json::{Value, json};
use std::io::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Latency samples per tuple (sequential request/echo round trips, one in flight).
const LATENCY_SAMPLES: usize = 200;
/// Warmup round trips per tuple, discarded.
const WARMUP_SAMPLES: usize = 20;
/// Throughput samples per tuple (sequential, so the number is request/echo-bound; see the docs).
const THROUGHPUT_SAMPLES: usize = 2_000;
/// Application payload sizes in bytes. The larger one stays inside the 1200-byte frame limit.
const PAYLOAD_SIZES: [usize; 2] = [64, 1_024];
/// The enforced sanity bounds: never absolute numbers, only these documented ratios.
const MAX_LATENCY_RATIO: f64 = 10.0;
const LATENCY_SLACK: Duration = Duration::from_millis(5);
const MIN_THROUGHPUT_FRACTION: f64 = 0.1;

const TCP_ALPN: &[u8] = b"baft-tcp-carrier-bench/1";

#[test]
fn benchmark_udp_against_tcp_and_write_artifact() -> Result<()> {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .expect("generate self-signed certificate");
    let certificate = CertificateDer::from(certified.cert.der().to_vec());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("build benchmark runtime");

    let mut tuples = Vec::new();
    let mut udp_max_datagram = None;
    for &payload in &PAYLOAD_SIZES {
        tuples.push(runtime.block_on(run_tcp_tuple(certificate.clone(), key.clone_key(), payload)));
        let (tuple, negotiated) =
            runtime.block_on(run_udp_tuple(certificate.clone(), key.clone_key(), payload));
        udp_max_datagram = Some(negotiated);
        tuples.push(tuple);
    }

    // Enforce the documented sanity bounds: a broken carrier must fail the harness, but the check
    // must not depend on this machine's absolute speed.
    let mut checks = Vec::new();
    for &payload in &PAYLOAD_SIZES {
        let tcp = tuple(&tuples, "tcp-tls+yamux", payload);
        let udp = tuple(&tuples, "udp-quic-datagram", payload);
        ensure!(
            tcp["verified_echoes"] == json!(LATENCY_SAMPLES + THROUGHPUT_SAMPLES),
            "TCP tuple for {payload} B did not complete every echo: {}",
            tcp["verified_echoes"]
        );
        ensure!(
            udp["verified_echoes"] == json!(LATENCY_SAMPLES + THROUGHPUT_SAMPLES),
            "UDP tuple for {payload} B did not complete every echo (datagram loss or a broken \
             carrier): {}",
            udp["verified_echoes"]
        );
        let tcp_p50 = tcp["latency_us"]["p50"].as_f64().expect("tcp p50");
        let udp_p50 = udp["latency_us"]["p50"].as_f64().expect("udp p50");
        let ceiling = (tcp_p50 * MAX_LATENCY_RATIO).max(tcp_p50 + LATENCY_SLACK.as_micros() as f64);
        let latency_ok = udp_p50 <= ceiling;
        ensure!(
            latency_ok,
            "UDP p50 {udp_p50:.1} us exceeds the documented sanity ceiling {ceiling:.1} us for \
             {payload} B (TCP p50 {tcp_p50:.1} us)"
        );
        let tcp_rate = tcp["throughput_bytes_per_sec"].as_f64().expect("tcp rate");
        let udp_rate = udp["throughput_bytes_per_sec"].as_f64().expect("udp rate");
        let throughput_ok = udp_rate >= tcp_rate * MIN_THROUGHPUT_FRACTION;
        ensure!(
            throughput_ok,
            "UDP throughput {udp_rate:.0} B/s is below the documented floor of \
             {MIN_THROUGHPUT_FRACTION} x TCP ({tcp_rate:.0} B/s) for {payload} B"
        );
        checks.push(json!({
            "payload_bytes": payload,
            "udp_p50_over_tcp_p50": ratio(udp_p50, tcp_p50),
            "latency_ceiling_us": ceiling,
            "latency_ok": latency_ok,
            "udp_throughput_over_tcp": ratio(udp_rate, tcp_rate),
            "throughput_ok": throughput_ok,
        }));
    }

    let artifact = json!({
        "schema": "baft-udp-carrier-bench/1",
        "generated_unix_ms": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        "toolchain": {
            "rustc": rustc_version(),
            "target": format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        },
        "environment": {
            "host": "loopback, single host, one process",
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "cpu_count": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        },
        "method": {
            "latency_samples": LATENCY_SAMPLES,
            "warmup_samples": WARMUP_SAMPLES,
            "throughput_samples": THROUGHPUT_SAMPLES,
            "payload_bytes": PAYLOAD_SIZES,
            "udp_negotiated_max_datagram": udp_max_datagram,
            "tcp_carrier": "TLS 1.3 (ring) + tokio-yamux with tunnel::yamux_config(8), one stream",
            "udp_carrier": "QUIC (quinn, ring) handshake + hello + RFC 9221 datagrams",
            "timestamp": "std::time::Instant (monotonic), microseconds",
            "throughput_definition": "payload bytes echoed / elapsed wall time, sequential request/echo",
        },
        "tuples": tuples,
        "enforced": {
            "echo_integrity": "every sample must return a byte-exact echo",
            "latency_rule": "udp p50 <= max(10 x tcp p50, tcp p50 + 5 ms)",
            "throughput_rule": "udp throughput >= 0.1 x tcp throughput",
            "checks": checks,
        },
        "reported_not_enforced": [
            "absolute p50/p95/p99 latency and throughput (machine-dependent)",
            "any real-path, loss, jitter or NAT-rebinding behaviour (gate (e), LAB)",
            "any profile tuple: ADR-0046 decision 10 leaves the profile-to-class mapping undecided",
        ],
    });

    let path = std::env::var("BAFT_UDP_BENCH_OUT")
        .unwrap_or_else(|_| "target/udp-carrier-bench.json".to_string());
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut file = std::fs::File::create(&path).expect("create benchmark artifact");
    serde_json::to_writer_pretty(&mut file, &artifact).expect("write benchmark artifact");
    file.flush().expect("flush benchmark artifact");
    println!("gate-(d) artifact: {path}");
    println!(
        "{}",
        serde_json::to_string(&artifact).expect("render artifact summary")
    );
    Ok(())
}

/// One tuple of the (carrier, payload size) matrix.
fn tuple<'a>(tuples: &'a [Value], carrier: &str, payload: usize) -> &'a Value {
    tuples
        .iter()
        .find(|t| t["carrier"] == json!(carrier) && t["payload_bytes"] == json!(payload))
        .expect("benchmark tuple present")
}

fn ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator <= 0.0 {
        0.0
    } else {
        (numerator / denominator * 1000.0).round() / 1000.0
    }
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn payload_of(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

fn latency_summary(mut samples: Vec<u64>) -> Value {
    samples.sort_unstable();
    let percentile = |p: f64| -> u64 {
        if samples.is_empty() {
            return 0;
        }
        let rank = ((p / 100.0) * samples.len() as f64).ceil() as usize;
        samples[rank.saturating_sub(1).min(samples.len() - 1)]
    };
    json!({
        "p50": percentile(50.0),
        "p95": percentile(95.0),
        "p99": percentile(99.0),
        "min": samples.first().copied().unwrap_or(0),
        "max": samples.last().copied().unwrap_or(0),
        "samples": samples.len(),
    })
}

fn tuple_result(carrier: &str, payload: usize, samples: Vec<u64>, rate: f64) -> Value {
    json!({
        "carrier": carrier,
        "payload_bytes": payload,
        "latency_us": latency_summary(samples),
        "throughput_bytes_per_sec": rate.round(),
        "verified_echoes": LATENCY_SAMPLES + THROUGHPUT_SAMPLES,
    })
}

// ---------------------------------------------------------------------------------------------
// TCP carrier arm: TLS 1.3 on ring with the pinned certificate, then the product's Yamux config.
// ---------------------------------------------------------------------------------------------

fn tls_server_config(
    certificate: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<rustls::ServerConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)?;
    config.alpn_protocols = vec![TCP_ALPN.to_vec()];
    Ok(config)
}

fn tls_client_config(certificate: CertificateDer<'static>) -> Result<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate)?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![TCP_ALPN.to_vec()];
    Ok(config)
}

async fn run_tcp_tuple(
    certificate: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    payload_size: usize,
) -> Value {
    let config = tls_server_config(certificate.clone(), key).expect("tcp server config");
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind tcp listener");
    let address = listener.local_addr().expect("tcp address");
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept tcp");
        let _ = tcp.set_nodelay(true);
        let tls = acceptor.accept(tcp).await.expect("tls accept");
        let mut session = tokio_yamux::Session::new_server(tls, yamux_config(8));
        while let Some(item) = session.next().await {
            let Ok(mut stream) = item else { break };
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buffer = vec![0_u8; payload_size];
                while stream.read_exact(&mut buffer).await.is_ok() {
                    if stream.write_all(&buffer).await.is_err() {
                        break;
                    }
                    let _ = stream.flush().await;
                }
            });
        }
    });

    let client_config = tls_client_config(certificate).expect("tcp client config");
    let connector = tokio_rustls::TlsConnector::from(Arc::new(client_config));
    let tcp = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect tcp");
    let _ = tcp.set_nodelay(true);
    let tls = connector
        .connect(ServerName::try_from("localhost").expect("server name"), tcp)
        .await
        .expect("tls connect");
    let mut session = tokio_yamux::Session::new_client(tls, yamux_config(8));
    let mut control = session.control();
    tokio::spawn(async move { while session.next().await.is_some() {} });
    let mut stream = control.open_stream().await.expect("open yamux stream");

    let payload = payload_of(payload_size);
    let mut echo = vec![0_u8; payload_size];

    for _ in 0..WARMUP_SAMPLES {
        tcp_round_trip(&mut stream, &payload, &mut echo).await;
    }
    let mut samples = Vec::with_capacity(LATENCY_SAMPLES);
    for _ in 0..LATENCY_SAMPLES {
        samples.push(tcp_round_trip(&mut stream, &payload, &mut echo).await);
    }
    let started = Instant::now();
    for _ in 0..THROUGHPUT_SAMPLES {
        tcp_round_trip(&mut stream, &payload, &mut echo).await;
    }
    let elapsed = started.elapsed();
    let rate = (payload_size * THROUGHPUT_SAMPLES) as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
    drop(stream);
    tuple_result("tcp-tls+yamux", payload_size, samples, rate)
}

// ---------------------------------------------------------------------------------------------
// UDP carrier arm: the real gate-(c) module — handshake, hello, datagrams.
// ---------------------------------------------------------------------------------------------

async fn run_udp_tuple(
    certificate: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    payload_size: usize,
) -> (Value, Option<usize>) {
    let config = carrier::server_config(certificate.clone(), key).expect("udp server config");
    let endpoint =
        quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap()).expect("bind udp endpoint");
    let address = endpoint.local_addr().expect("udp address");
    let server = tokio::spawn(echo_udp_peer(endpoint));

    let client_config = carrier::client_config(certificate).expect("udp client config");
    let endpoint =
        quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).expect("bind udp client endpoint");
    let connection = tokio::time::timeout(
        Duration::from_secs(20),
        carrier::connect(&endpoint, client_config, address),
    )
    .await
    .expect("udp connect timed out")
    .expect("udp connect failed");
    carrier::send_hello(&connection, "client", [11_u8; carrier::NONCE_LEN])
        .await
        .expect("udp hello exchange failed");
    let negotiated = connection.max_datagram_size();

    let payload = payload_of(payload_size);
    let mut sequence = 0_u64;
    for _ in 0..WARMUP_SAMPLES {
        udp_round_trip(&connection, &mut sequence, &payload).await;
    }
    let mut samples = Vec::with_capacity(LATENCY_SAMPLES);
    for _ in 0..LATENCY_SAMPLES {
        samples.push(udp_round_trip(&connection, &mut sequence, &payload).await);
    }
    let started = Instant::now();
    for _ in 0..THROUGHPUT_SAMPLES {
        udp_round_trip(&connection, &mut sequence, &payload).await;
    }
    let elapsed = started.elapsed();
    let rate = (payload_size * THROUGHPUT_SAMPLES) as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
    connection.close(0_u32.into(), b"benchmark done");
    server.abort();
    (
        tuple_result("udp-quic-datagram", payload_size, samples, rate),
        negotiated,
    )
}

async fn tcp_round_trip(
    stream: &mut tokio_yamux::StreamHandle,
    payload: &[u8],
    echo: &mut [u8],
) -> u64 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let started = Instant::now();
    stream.write_all(payload).await.expect("write tcp message");
    stream.flush().await.expect("flush tcp message");
    stream.read_exact(echo).await.expect("read tcp echo");
    assert_eq!(echo, payload, "tcp echo mismatch");
    started.elapsed().as_micros() as u64
}

/// One datagram there and back, verified byte-exact; returns the round-trip in microseconds.
async fn udp_round_trip(connection: &quinn::Connection, sequence: &mut u64, payload: &[u8]) -> u64 {
    *sequence += 1;
    let sent = *sequence;
    let started = Instant::now();
    carrier::send_datagram(connection, sent, payload)
        .await
        .expect("send udp datagram");
    let (received, echo) =
        tokio::time::timeout(Duration::from_secs(5), carrier::recv_datagram(connection))
            .await
            .expect("udp datagram echo timed out")
            .expect("udp datagram echo failed");
    let elapsed = started.elapsed().as_micros() as u64;
    assert_eq!(received, sent, "udp echo sequence mismatch");
    assert_eq!(echo, payload, "udp echo mismatch");
    elapsed
}

async fn echo_udp_peer(endpoint: quinn::Endpoint) {
    let connection = carrier::accept(&endpoint).await.expect("udp accept failed");
    carrier::answer_hello(&connection, "server")
        .await
        .expect("udp hello answer failed");
    while let Ok((sequence, payload)) = carrier::recv_datagram(&connection).await {
        if carrier::send_datagram(&connection, sequence, &payload)
            .await
            .is_err()
        {
            break;
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), connection.closed()).await;
    endpoint.close(0_u32.into(), b"benchmark done");
}
