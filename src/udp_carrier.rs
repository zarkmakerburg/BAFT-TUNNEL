//! Prototype UDP/QUIC datagram carrier (ADR-0046), behind the non-default `udp-carrier` feature.
//!
//! This module is the gate-(c) surface only: a small fixed datagram frame, a bounded control-plane
//! hello, and the QUIC endpoint/connection helpers the fuzz target and the two-process loopback test
//! drive. It purposely has no CLI verb, config key, policy field or capability claim: no
//! user-facing surface exists before gates (b) and (e), and `transport.udp` stays `Unknown`.
//!
//! Datagrams are unreliable and unordered by design (ADR-0046 decision 5); nothing here
//! retransmits or reorders, and the payload stays within the 1200-byte assumed path MTU.

use anyhow::{Context, Result, ensure};
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

/// ALPN for the prototype carrier. A distinct value so a misdirected client fails the handshake.
pub const ALPN: &[u8] = b"baft-udp-carrier/1";
/// Assumed minimum path MTU; the application never fragments (ADR-0046 decision 5).
pub const MAX_DATAGRAM: usize = 1200;
/// Bounded control-plane hello; anything longer is refused before parsing further.
pub const MAX_HELLO: usize = 128;
/// Nonce length carried by the prototype hello.
pub const NONCE_LEN: usize = 16;
/// Wire version of both the datagram frame and the hello.
pub const PROTOCOL_VERSION: u8 = 1;

const DATAGRAM_MAGIC: &[u8; 4] = b"BUD1";
const HELLO_MAGIC: &[u8; 4] = b"BUH1";
/// magic(4) + version(1) + sequence(8) + payload length(2).
const DATAGRAM_HEADER_LEN: usize = 15;

/// A parsed application datagram frame.
#[derive(Debug, PartialEq, Eq)]
pub struct DecodedDatagram<'a> {
    pub sequence: u64,
    pub payload: &'a [u8],
}

/// A parsed control-plane hello.
#[derive(Debug, PartialEq, Eq)]
pub struct DecodedHello<'a> {
    pub peer_id: &'a str,
    pub nonce: [u8; NONCE_LEN],
}

/// Encode one unreliable datagram: `BUD1 | version | sequence | length | payload`.
///
/// Fails closed on an oversized payload instead of fragmenting it.
pub fn encode_datagram(sequence: u64, payload: &[u8]) -> Result<Vec<u8>> {
    let total = DATAGRAM_HEADER_LEN
        .checked_add(payload.len())
        .context("datagram length overflow")?;
    ensure!(
        total <= MAX_DATAGRAM,
        "datagram of {total} bytes exceeds the {MAX_DATAGRAM}-byte carrier limit"
    );
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(DATAGRAM_MAGIC);
    out.push(PROTOCOL_VERSION);
    out.extend_from_slice(&sequence.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Parse one application datagram. Every malformed shape is refused; no trailing bytes are
/// accepted, so a frame boundary is never guessed.
pub fn decode_datagram(bytes: &[u8]) -> Result<DecodedDatagram<'_>> {
    ensure!(
        bytes.len() >= DATAGRAM_HEADER_LEN,
        "datagram shorter than the {DATAGRAM_HEADER_LEN}-byte header"
    );
    ensure!(&bytes[..4] == DATAGRAM_MAGIC, "bad datagram magic");
    ensure!(
        bytes[4] == PROTOCOL_VERSION,
        "unsupported datagram version {}",
        bytes[4]
    );
    let sequence = u64::from_be_bytes(bytes[5..13].try_into().expect("fixed 8-byte slice"));
    let length = u16::from_be_bytes([bytes[13], bytes[14]]) as usize;
    ensure!(
        length <= MAX_DATAGRAM - DATAGRAM_HEADER_LEN,
        "declared payload length {length} exceeds the carrier limit"
    );
    ensure!(
        bytes.len() == DATAGRAM_HEADER_LEN + length,
        "declared payload length {length} does not match {} bytes",
        bytes.len() - DATAGRAM_HEADER_LEN
    );
    Ok(DecodedDatagram {
        sequence,
        payload: &bytes[DATAGRAM_HEADER_LEN..],
    })
}

fn validate_peer_id(peer_id: &str) -> Result<()> {
    ensure!(
        !peer_id.is_empty() && peer_id.len() <= 64,
        "peer id must be 1-64 bytes"
    );
    ensure!(
        peer_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b)),
        "peer id must be ASCII letters, digits, '.', '_', '-' or ':'"
    );
    Ok(())
}

/// Encode the bounded control-plane hello: `BUH1 | version | id length | id | nonce`.
pub fn encode_hello(peer_id: &str, nonce: &[u8; NONCE_LEN]) -> Result<Vec<u8>> {
    validate_peer_id(peer_id)?;
    let total = 6 + peer_id.len() + NONCE_LEN;
    ensure!(total <= MAX_HELLO, "hello of {total} bytes is too long");
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(HELLO_MAGIC);
    out.push(PROTOCOL_VERSION);
    out.push(peer_id.len() as u8);
    out.extend_from_slice(peer_id.as_bytes());
    out.extend_from_slice(nonce);
    Ok(out)
}

/// Parse the bounded control-plane hello; malformed ids, versions, lengths and trailing bytes are
/// refused before any state is built.
pub fn decode_hello(bytes: &[u8]) -> Result<DecodedHello<'_>> {
    ensure!(bytes.len() >= 6 + NONCE_LEN, "hello is too short");
    ensure!(&bytes[..4] == HELLO_MAGIC, "bad hello magic");
    ensure!(
        bytes[4] == PROTOCOL_VERSION,
        "unsupported hello version {}",
        bytes[4]
    );
    let id_len = bytes[5] as usize;
    ensure!(
        id_len > 0 && id_len <= 64,
        "hello peer id length is invalid"
    );
    ensure!(
        bytes.len() == 6 + id_len + NONCE_LEN,
        "hello length does not match its peer id length"
    );
    let peer_id =
        std::str::from_utf8(&bytes[6..6 + id_len]).context("hello peer id is not UTF-8")?;
    validate_peer_id(peer_id)?;
    let mut nonce = [0_u8; NONCE_LEN];
    nonce.copy_from_slice(&bytes[6 + id_len..]);
    Ok(DecodedHello { peer_id, nonce })
}

fn transport_config() -> quinn::TransportConfig {
    let mut transport = quinn::TransportConfig::default();
    // Datagrams are optional in QUIC; make the buffer explicit rather than relying on defaults.
    transport.datagram_receive_buffer_size(Some(256 * 1024));
    transport.datagram_send_buffer_size(256 * 1024);
    transport.max_idle_timeout(Some(
        Duration::from_secs(20)
            .try_into()
            .expect("valid idle timeout"),
    ));
    transport
}

/// Server config for the prototype: one pinned certificate, ring only, ALPN fixed, datagrams on.
pub fn server_config(
    certificate: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<quinn::ServerConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut crypto = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)?;
    crypto.alpn_protocols = vec![ALPN.to_vec()];
    let quic = QuicServerConfig::try_from(crypto)?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(quic));
    config.transport_config(Arc::new(transport_config()));
    Ok(config)
}

/// Client config for the prototype: the server certificate is the only trust anchor, ring only.
pub fn client_config(certificate: CertificateDer<'static>) -> Result<quinn::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate)?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut crypto = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    crypto.alpn_protocols = vec![ALPN.to_vec()];
    let quic = QuicClientConfig::try_from(crypto)?;
    let mut config = quinn::ClientConfig::new(Arc::new(quic));
    config.transport_config(Arc::new(transport_config()));
    Ok(config)
}

/// Accept exactly one peer and complete the QUIC handshake.
pub async fn accept(endpoint: &quinn::Endpoint) -> Result<quinn::Connection> {
    let incoming = endpoint
        .accept()
        .await
        .context("carrier endpoint closed before a peer connected")?;
    incoming.await.context("QUIC handshake failed")
}

/// Connect to one peer and complete the QUIC handshake.
pub async fn connect(
    endpoint: &quinn::Endpoint,
    config: quinn::ClientConfig,
    remote: SocketAddr,
) -> Result<quinn::Connection> {
    endpoint
        .connect_with(config, remote, "localhost")?
        .await
        .context("QUIC handshake failed")
}

/// Client side of the control-plane hello: send ours on a bidirectional stream, read the reply.
pub async fn send_hello(
    connection: &quinn::Connection,
    peer_id: &str,
    nonce: [u8; NONCE_LEN],
) -> Result<()> {
    let (mut send, mut recv) = connection.open_bi().await.context("open hello stream")?;
    send.write_all(&encode_hello(peer_id, &nonce)?)
        .await
        .context("write hello")?;
    send.finish().context("finish hello stream")?;
    let reply = recv
        .read_to_end(MAX_HELLO)
        .await
        .context("read hello reply")?;
    let hello = decode_hello(&reply)?;
    ensure!(
        hello.peer_id == "server",
        "unexpected hello peer id {}",
        hello.peer_id
    );
    // The reply must echo this call's nonce. Without this check the nonce is transmitted but binds
    // nothing, so a recorded hello reply would be accepted by a caller that never sent it.
    ensure!(
        hello.nonce == nonce,
        "hello reply did not echo this client's nonce"
    );
    Ok(())
}

/// Server side of the control-plane hello: read and validate the peer's, then answer with ours,
/// echoing the peer's nonce so the caller can tell a live answer from a recorded one.
pub async fn answer_hello(connection: &quinn::Connection, peer_id: &str) -> Result<()> {
    let (mut send, mut recv) = connection
        .accept_bi()
        .await
        .context("accept hello stream")?;
    let request = recv.read_to_end(MAX_HELLO).await.context("read hello")?;
    let hello = decode_hello(&request)?;
    ensure!(
        hello.peer_id == "client",
        "unexpected hello peer id {}",
        hello.peer_id
    );
    send.write_all(&encode_hello(peer_id, &hello.nonce)?)
        .await
        .context("write hello reply")?;
    send.finish().context("finish hello reply")?;
    Ok(())
}

/// Send one unreliable datagram, refusing anything the negotiated path cannot carry.
pub async fn send_datagram(
    connection: &quinn::Connection,
    sequence: u64,
    payload: &[u8],
) -> Result<()> {
    let frame = encode_datagram(sequence, payload)?;
    let max = connection
        .max_datagram_size()
        .context("peer did not negotiate the datagram extension")?;
    ensure!(
        frame.len() <= max,
        "frame of {} bytes exceeds the negotiated {max}-byte datagram size",
        frame.len()
    );
    connection.send_datagram(frame.into())?;
    Ok(())
}

/// Receive one datagram and parse its frame.
pub async fn recv_datagram(connection: &quinn::Connection) -> Result<(u64, Vec<u8>)> {
    let datagram = connection.read_datagram().await?;
    let decoded = decode_datagram(&datagram)?;
    Ok((decoded.sequence, decoded.payload.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::PrivatePkcs8KeyDer;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::Instant;

    const TEST_NAME: &str = "udp_carrier::tests::two_process_loopback";
    const ROLE_ENV: &str = "BAFT_UDP_CARRIER_TEST_ROLE";
    const DIR_ENV: &str = "BAFT_UDP_CARRIER_TEST_DIR";

    #[test]
    fn datagram_and_hello_round_trip_and_refuse_malformed_input() {
        let frame = encode_datagram(7, b"payload").unwrap();
        assert_eq!(
            decode_datagram(&frame).unwrap(),
            DecodedDatagram {
                sequence: 7,
                payload: b"payload"
            }
        );
        assert!(decode_datagram(&frame[..frame.len() - 1]).is_err());
        assert!(decode_datagram(&[frame.as_slice(), b"x"].concat()).is_err());
        assert!(encode_datagram(0, &[0; MAX_DATAGRAM]).is_err());

        let nonce = [3_u8; NONCE_LEN];
        let hello = encode_hello("client", &nonce).unwrap();
        assert_eq!(
            decode_hello(&hello).unwrap(),
            DecodedHello {
                peer_id: "client",
                nonce
            }
        );
        assert!(decode_hello(&hello[..hello.len() - 1]).is_err());
        assert!(encode_hello("", &nonce).is_err());
        assert!(encode_hello(&"a".repeat(65), &nonce).is_err());
    }

    /// Gate (c): two real processes, a real QUIC handshake, the control-plane hello and one
    /// datagram there and back. No `#[ignore]` and no skip: with `udp-carrier` enabled this test
    /// fails if any step does not complete.
    #[test]
    fn two_process_loopback() {
        let role = std::env::var(ROLE_ENV).ok();
        match role.as_deref() {
            Some("server") => run_server_child(),
            Some("client") => run_client_child(),
            Some(other) => panic!("unknown {ROLE_ENV} value {other}"),
            None => run_parent(),
        }
    }

    fn run_parent() {
        let dir = std::env::temp_dir().join(format!(
            "baft-udp-carrier-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create test directory");
        let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("generate test certificate");
        std::fs::write(dir.join("cert.der"), certified.cert.der().as_ref())
            .expect("write certificate");
        std::fs::write(dir.join("key.der"), certified.signing_key.serialize_der())
            .expect("write key");

        let exe = std::env::current_exe().expect("test executable path");
        let mut server = spawn_role(&exe, "server", &dir);
        let addr_path = dir.join("addr");
        let deadline = Instant::now() + Duration::from_secs(20);
        while !addr_path.exists() {
            if let Some(status) = server.try_wait().expect("poll server") {
                let output = server.wait_with_output().expect("collect server output");
                panic!(
                    "server child exited before publishing its address: {status}\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            assert!(
                Instant::now() < deadline,
                "server child did not publish an address within 20 s"
            );
            std::thread::sleep(Duration::from_millis(25));
        }

        let client = spawn_role(&exe, "client", &dir);
        let server_output = server.wait_with_output().expect("wait for server child");
        let client_output = client.wait_with_output().expect("wait for client child");
        assert!(
            server_output.status.success(),
            "server child failed: {}\nstdout:\n{}\nstderr:\n{}",
            server_output.status,
            String::from_utf8_lossy(&server_output.stdout),
            String::from_utf8_lossy(&server_output.stderr)
        );
        assert!(
            client_output.status.success(),
            "client child failed: {}\nstdout:\n{}\nstderr:\n{}",
            client_output.status,
            String::from_utf8_lossy(&client_output.stdout),
            String::from_utf8_lossy(&client_output.stderr)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn spawn_role(exe: &Path, role: &str, dir: &Path) -> Child {
        Command::new(exe)
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(ROLE_ENV, role)
            .env(DIR_ENV, dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn child process")
    }

    fn test_dir() -> PathBuf {
        PathBuf::from(std::env::var(DIR_ENV).expect("child test directory"))
    }

    fn read_certificate(dir: &Path) -> CertificateDer<'static> {
        CertificateDer::from(std::fs::read(dir.join("cert.der")).expect("read certificate"))
    }

    fn read_key(dir: &Path) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            std::fs::read(dir.join("key.der")).expect("read key"),
        ))
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build test runtime")
    }

    fn run_server_child() {
        let dir = test_dir();
        let config = server_config(read_certificate(&dir), read_key(&dir)).expect("server config");
        runtime().block_on(async {
            let endpoint = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap())
                .expect("bind server endpoint");
            let addr = endpoint.local_addr().expect("server address");
            std::fs::write(dir.join("addr"), addr.to_string()).expect("publish server address");

            let connection = tokio::time::timeout(Duration::from_secs(20), accept(&endpoint))
                .await
                .expect("accept timed out")
                .expect("accept failed");
            answer_hello(&connection, "server")
                .await
                .expect("hello exchange failed");
            let (sequence, payload) =
                tokio::time::timeout(Duration::from_secs(20), recv_datagram(&connection))
                    .await
                    .expect("datagram receive timed out")
                    .expect("datagram receive failed");
            assert_eq!(sequence, 42);
            assert_eq!(payload, b"ping");
            send_datagram(&connection, sequence, b"pong")
                .await
                .expect("datagram reply failed");
            let _ = tokio::time::timeout(Duration::from_secs(10), connection.closed()).await;
            endpoint.close(0_u32.into(), b"done");
        });
    }

    fn run_client_child() {
        let dir = test_dir();
        let address: SocketAddr = std::fs::read_to_string(dir.join("addr"))
            .expect("read server address")
            .trim()
            .parse()
            .expect("parse server address");
        let config = client_config(read_certificate(&dir)).expect("client config");
        runtime().block_on(async {
            let endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap())
                .expect("bind client endpoint");
            let connection =
                tokio::time::timeout(Duration::from_secs(20), connect(&endpoint, config, address))
                    .await
                    .expect("connect timed out")
                    .expect("connect failed");
            assert_eq!(connection.remote_address(), address);
            send_hello(&connection, "client", [9_u8; NONCE_LEN])
                .await
                .expect("hello exchange failed");
            assert!(
                connection.max_datagram_size().is_some(),
                "the handshake did not negotiate the datagram extension"
            );
            send_datagram(&connection, 42, b"ping")
                .await
                .expect("datagram send failed");
            let (sequence, payload) =
                tokio::time::timeout(Duration::from_secs(20), recv_datagram(&connection))
                    .await
                    .expect("datagram echo timed out")
                    .expect("datagram echo failed");
            assert_eq!(sequence, 42);
            assert_eq!(payload, b"pong");
            connection.close(0_u32.into(), b"done");
            endpoint.wait_idle().await;
        });
    }

    // Gate (f): fail-closed and DoS behaviour that a test can pin. The analysis, including every
    // claim these tests do not establish, is docs/TP_U_FAIL_CLOSED_AND_DOS.md.

    fn certified() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
        let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("generate test certificate");
        (
            CertificateDer::from(certified.cert.der().to_vec()),
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                certified.signing_key.serialize_der(),
            )),
        )
    }

    /// A handshake that cannot verify the peer fails closed: no connection exists, so nothing can
    /// be sent on the data plane, and there is no fallback carrier to silently switch to.
    #[test]
    fn handshake_failure_is_fail_closed() {
        let (server_cert, server_key) = certified();
        let (other_cert, _other_key) = certified();
        runtime().block_on(async move {
            let config = server_config(server_cert, server_key).expect("server config");
            let endpoint = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap())
                .expect("bind server");
            let address = endpoint.local_addr().expect("server address");
            let client_config = client_config(other_cert).expect("client config");
            let client_endpoint =
                quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).expect("bind client");
            // Fail closed means no session is obtained: the attempt errors or is still unresolved
            // when the bound expires. A successful connection is the only failing outcome.
            let outcome = tokio::time::timeout(
                Duration::from_secs(2),
                connect(&client_endpoint, client_config, address),
            )
            .await;
            if let Ok(Ok(_)) = outcome {
                panic!("a client that trusts another certificate must not connect");
            }
            client_endpoint.close(0_u32.into(), b"done");
            endpoint.close(0_u32.into(), b"done");
        });
    }

    /// A closed session fails reads instead of hanging, so a caller cannot mistake a dead carrier
    /// for a slow one and wait forever.
    #[test]
    fn closed_session_fails_reads_instead_of_hanging() {
        let (certificate, key) = certified();
        runtime().block_on(async move {
            let config = server_config(certificate.clone(), key).expect("server config");
            let endpoint = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap())
                .expect("bind server");
            let address = endpoint.local_addr().expect("server address");
            let (tx, rx) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                let connection = accept(&endpoint).await.expect("accept");
                answer_hello(&connection, "server").await.expect("hello");
                let outcome =
                    tokio::time::timeout(Duration::from_secs(10), recv_datagram(&connection)).await;
                let _ = tx.send(matches!(outcome, Ok(Err(_))));
                endpoint.close(0_u32.into(), b"done");
            });
            let client_config = client_config(certificate).expect("client config");
            let client_endpoint =
                quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).expect("bind client");
            let connection = connect(&client_endpoint, client_config, address)
                .await
                .expect("connect");
            send_hello(&connection, "client", [2_u8; NONCE_LEN])
                .await
                .expect("hello");
            connection.close(0_u32.into(), b"client done");
            let failed_closed = tokio::time::timeout(Duration::from_secs(10), rx)
                .await
                .expect("server read did not finish within 10 s")
                .expect("server task result");
            assert!(
                failed_closed,
                "a read on a closed session must return an error, not wait"
            );
            client_endpoint.close(0_u32.into(), b"done");
        });
    }

    /// Malformed application frames and oversize sends are refused per message: the session stays
    /// usable, so a hostile or buggy peer cannot end a healthy carrier with one bad datagram.
    #[test]
    fn malformed_and_oversize_datagrams_fail_closed_per_message() {
        let (certificate, key) = certified();
        runtime().block_on(async move {
            let config = server_config(certificate.clone(), key).expect("server config");
            let endpoint = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap())
                .expect("bind server");
            let address = endpoint.local_addr().expect("server address");
            tokio::spawn(async move {
                let connection = accept(&endpoint).await.expect("accept");
                answer_hello(&connection, "server").await.expect("hello");
                loop {
                    match recv_datagram(&connection).await {
                        Ok((sequence, payload)) => {
                            send_datagram(&connection, sequence, &payload)
                                .await
                                .expect("echo");
                        }
                        Err(_) => continue,
                    }
                }
            });
            let client_config = client_config(certificate).expect("client config");
            let client_endpoint =
                quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).expect("bind client");
            let connection = connect(&client_endpoint, client_config, address)
                .await
                .expect("connect");
            send_hello(&connection, "client", [4_u8; NONCE_LEN])
                .await
                .expect("hello");

            connection
                .send_datagram(Vec::from(b"not-a-frame").into())
                .expect("a hostile raw datagram is sent to the peer");
            assert!(
                send_datagram(&connection, 1, &[0_u8; MAX_DATAGRAM])
                    .await
                    .is_err(),
                "an oversized frame must be refused before it reaches the wire"
            );

            send_datagram(&connection, 9, b"still-here")
                .await
                .expect("send after refusals");
            let (sequence, payload) =
                tokio::time::timeout(Duration::from_secs(10), recv_datagram(&connection))
                    .await
                    .expect("echo timed out")
                    .expect("echo failed");
            assert_eq!(sequence, 9);
            assert_eq!(payload, b"still-here");
            connection.close(0_u32.into(), b"done");
            client_endpoint.close(0_u32.into(), b"done");
        });
    }

    /// A hello reply that does not echo the caller's nonce must be refused: the nonce exists to
    /// bind the answer to this exchange, so a recorded reply cannot be presented to a new caller.
    #[test]
    fn hello_reply_must_echo_the_client_nonce() {
        let (certificate, key) = certified();
        runtime().block_on(async move {
            let config = server_config(certificate.clone(), key).expect("server config");
            let endpoint = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap())
                .expect("bind server");
            let address = endpoint.local_addr().expect("server address");
            tokio::spawn(async move {
                let connection = accept(&endpoint).await.expect("accept");
                let (mut send, mut recv) =
                    connection.accept_bi().await.expect("accept hello stream");
                let request = recv.read_to_end(MAX_HELLO).await.expect("read hello");
                decode_hello(&request).expect("client hello");
                // Answer with a nonce this client never sent, i.e. a stale or recorded reply.
                send.write_all(&encode_hello("server", &[0xaa; NONCE_LEN]).expect("encode"))
                    .await
                    .expect("write hello reply");
                send.finish().expect("finish hello reply");
                let _ = tokio::time::timeout(Duration::from_secs(10), connection.closed()).await;
                endpoint.close(0_u32.into(), b"done");
            });
            let client_config = client_config(certificate).expect("client config");
            let client_endpoint =
                quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).expect("bind client");
            let connection = connect(&client_endpoint, client_config, address)
                .await
                .expect("connect");
            let outcome = send_hello(&connection, "client", [0x11; NONCE_LEN]).await;
            assert!(
                outcome.is_err(),
                "a hello reply that does not echo this client's nonce must be refused"
            );
            client_endpoint.close(0_u32.into(), b"done");
        });
    }

    /// Datagrams are never retransmitted: what the peer receives is what arrived once, so a lost
    /// datagram is counted as loss and never replayed behind the application's back.
    #[test]
    fn datagram_loss_is_not_retransmitted() {
        let (certificate, key) = certified();
        runtime().block_on(async move {
            let config = server_config(certificate.clone(), key).expect("server config");
            let endpoint = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap())
                .expect("bind server");
            let address = endpoint.local_addr().expect("server address");
            let (tx, rx) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                let connection = accept(&endpoint).await.expect("accept");
                answer_hello(&connection, "server").await.expect("hello");
                let mut sequences = Vec::new();
                while let Ok((sequence, _payload)) = recv_datagram(&connection).await {
                    sequences.push(sequence);
                }
                let _ = tx.send(sequences);
                endpoint.close(0_u32.into(), b"done");
            });
            let client_config = client_config(certificate).expect("client config");
            let client_endpoint =
                quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).expect("bind client");
            let connection = connect(&client_endpoint, client_config, address)
                .await
                .expect("connect");
            send_hello(&connection, "client", [6_u8; NONCE_LEN])
                .await
                .expect("hello");
            for sequence in 1..=3_u64 {
                send_datagram(&connection, sequence, b"one-shot")
                    .await
                    .expect("send");
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            connection.close(0_u32.into(), b"done");
            let sequences = tokio::time::timeout(Duration::from_secs(10), rx)
                .await
                .expect("server did not finish")
                .expect("server task result");
            assert!(sequences.len() <= 3, "the peer saw a retransmission");
            let mut unique = sequences.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), sequences.len(), "a sequence arrived twice");
            client_endpoint.close(0_u32.into(), b"done");
        });
    }
}
