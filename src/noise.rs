//! Noise IK transport over length-prefixed TCP records.
//!
//! This module is the cryptographic stream adapter for the reverse-TCP data plane.
//! Callers must authenticate the resulting static key against the enrolled peer
//! record and bind identities/tunnel/version into the supplied prologue.

use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use anyhow::{Context as AnyhowContext, Result, bail};
use snow::{Builder, HandshakeState, TransportState, params::NoiseParams};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream},
    sync::Mutex,
};

pub const NOISE_PATTERN: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";
const MAX_HANDSHAKE_RECORD: usize = 65_535;
const MAX_PLAINTEXT_RECORD: usize = 16 * 1024;
const MAX_CIPHERTEXT_RECORD: usize = MAX_PLAINTEXT_RECORD + 16;
const BRIDGE_CAPACITY: usize = 64 * 1024;

pub struct NoiseIo {
    inner: DuplexStream,
}

impl AsyncRead for NoiseIo {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buffer)
    }
}

impl AsyncWrite for NoiseIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, bytes)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

fn params() -> Result<NoiseParams> {
    NOISE_PATTERN
        .parse()
        .context("parse fixed Noise protocol suite")
}

pub fn generate_static_keypair() -> Result<snow::Keypair> {
    Builder::new(params()?)
        .generate_keypair()
        .context("generate Noise static keypair")
}

async fn write_record<T: AsyncWrite + Unpin>(io: &mut T, record: &[u8]) -> Result<()> {
    if record.is_empty() || record.len() > MAX_HANDSHAKE_RECORD {
        bail!("Noise record length is outside the permitted range");
    }
    let length = u32::try_from(record.len())?;
    io.write_all(&length.to_be_bytes()).await?;
    io.write_all(record).await?;
    io.flush().await?;
    Ok(())
}

async fn read_record<T: AsyncRead + Unpin>(io: &mut T, maximum: usize) -> Result<Vec<u8>> {
    let mut prefix = [0_u8; 4];
    io.read_exact(&mut prefix).await?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > maximum {
        bail!("Noise record length is outside the permitted range");
    }
    let mut record = vec![0_u8; length];
    io.read_exact(&mut record).await?;
    Ok(record)
}

async fn send_handshake<T: AsyncWrite + Unpin>(
    io: &mut T,
    state: &mut HandshakeState,
    payload: &[u8],
) -> Result<()> {
    let mut message = vec![0_u8; MAX_HANDSHAKE_RECORD];
    let length = state
        .write_message(payload, &mut message)
        .context("write Noise handshake message")?;
    message.truncate(length);
    write_record(io, &message).await
}

async fn receive_handshake<T: AsyncRead + Unpin>(
    io: &mut T,
    state: &mut HandshakeState,
) -> Result<Vec<u8>> {
    let message = read_record(io, MAX_HANDSHAKE_RECORD).await?;
    let mut payload = vec![0_u8; MAX_HANDSHAKE_RECORD];
    let length = state
        .read_message(&message, &mut payload)
        .context("authenticate Noise handshake message")?;
    payload.truncate(length);
    Ok(payload)
}

pub async fn initiate<T>(
    mut io: T,
    local_private: &[u8],
    pinned_remote_static: &[u8],
    prologue: &[u8],
) -> Result<NoiseIo>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut state = Builder::new(params()?)
        .local_private_key(local_private)
        .remote_public_key(pinned_remote_static)
        .prologue(prologue)
        .build_initiator()
        .context("build Noise IK initiator")?;

    send_handshake(&mut io, &mut state, &[]).await?;
    receive_handshake(&mut io, &mut state).await?;

    let observed = state
        .get_remote_static()
        .context("Noise responder did not authenticate a static key")?;
    if observed != pinned_remote_static {
        bail!("Noise responder static key does not match the pinned peer key");
    }
    if !state.is_handshake_finished() {
        bail!("Noise IK handshake did not complete");
    }
    bridge_transport(
        io,
        state
            .into_transport_mode()
            .context("enter Noise transport mode")?,
    )
}

pub async fn accept<T>(
    mut io: T,
    local_private: &[u8],
    expected_initiator_static: &[u8],
    prologue: &[u8],
) -> Result<NoiseIo>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut state = Builder::new(params()?)
        .local_private_key(local_private)
        .prologue(prologue)
        .build_responder()
        .context("build Noise IK responder")?;

    receive_handshake(&mut io, &mut state).await?;
    let observed = state
        .get_remote_static()
        .context("Noise initiator did not authenticate a static key")?;
    if observed != expected_initiator_static {
        bail!("Noise initiator static key is not enrolled for this peer");
    }
    send_handshake(&mut io, &mut state, &[]).await?;
    if !state.is_handshake_finished() {
        bail!("Noise IK handshake did not complete");
    }
    bridge_transport(
        io,
        state
            .into_transport_mode()
            .context("enter Noise transport mode")?,
    )
}

fn bridge_transport<T>(io: T, transport_state: TransportState) -> Result<NoiseIo>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut network_read, mut network_write) = tokio::io::split(io);
    let (application, bridge) = tokio::io::duplex(BRIDGE_CAPACITY);
    let (mut bridge_read, mut bridge_write) = tokio::io::split(bridge);
    let state = Arc::new(Mutex::new(transport_state));

    let read_state = Arc::clone(&state);
    tokio::spawn(async move {
        loop {
            let record = match read_record(&mut network_read, MAX_CIPHERTEXT_RECORD).await {
                Ok(record) => record,
                Err(_) => break,
            };
            let mut plaintext = vec![0_u8; MAX_PLAINTEXT_RECORD];
            let length = {
                let mut state = read_state.lock().await;
                match state.read_message(&record, &mut plaintext) {
                    Ok(length) => length,
                    Err(_) => break,
                }
            };
            if bridge_write.write_all(&plaintext[..length]).await.is_err() {
                break;
            }
        }
        let _ = bridge_write.shutdown().await;
    });

    let write_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut plaintext = vec![0_u8; MAX_PLAINTEXT_RECORD];
        loop {
            let length = match bridge_read.read(&mut plaintext).await {
                Ok(0) | Err(_) => break,
                Ok(length) => length,
            };
            let mut ciphertext = vec![0_u8; MAX_CIPHERTEXT_RECORD];
            let encrypted = {
                let mut state = write_state.lock().await;
                match state.write_message(&plaintext[..length], &mut ciphertext) {
                    Ok(length) => length,
                    Err(_) => break,
                }
            };
            if write_record(&mut network_write, &ciphertext[..encrypted])
                .await
                .is_err()
            {
                break;
            }
        }
        let _ = network_write.shutdown().await;
    });

    Ok(NoiseIo { inner: application })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt, duplex},
        time::{Duration, timeout},
    };

    #[tokio::test]
    async fn ik_authenticates_both_static_keys_and_carries_encrypted_stream_data() {
        let initiator = generate_static_keypair().unwrap();
        let responder = generate_static_keypair().unwrap();
        let (left, right) = duplex(128 * 1024);
        let prologue = b"baft-tunnel/data-plane/v1|node-a|node-b|tunnel-1";

        let server = tokio::spawn(async move {
            let mut secure = accept(right, &responder.private, &initiator.public, prologue)
                .await
                .unwrap();
            let mut received = [0_u8; 11];
            secure.read_exact(&mut received).await.unwrap();
            assert_eq!(&received, b"hello noise");
            secure.write_all(b"reply").await.unwrap();
        });

        let mut secure = timeout(
            Duration::from_secs(5),
            initiate(left, &initiator.private, &responder.public, prologue),
        )
        .await
        .unwrap()
        .unwrap();
        secure.write_all(b"hello noise").await.unwrap();
        let mut response = [0_u8; 5];
        timeout(Duration::from_secs(5), secure.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response, b"reply");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn ik_rejects_wrong_enrolled_initiator_key() {
        let initiator = generate_static_keypair().unwrap();
        let responder = generate_static_keypair().unwrap();
        let wrong_peer = generate_static_keypair().unwrap();
        let (left, right) = duplex(128 * 1024);
        let server = tokio::spawn(async move {
            accept(
                right,
                &responder.private,
                &wrong_peer.public,
                b"baft-test/v1",
            )
            .await
            .is_err()
        });
        let client = initiate(left, &initiator.private, &responder.public, b"baft-test/v1").await;
        assert!(client.is_err() || server.await.unwrap());
    }

    #[tokio::test]
    async fn ik_rejects_altered_prologue() {
        let initiator = generate_static_keypair().unwrap();
        let responder = generate_static_keypair().unwrap();
        let (left, right) = duplex(128 * 1024);
        let server = tokio::spawn(async move {
            accept(
                right,
                &responder.private,
                &initiator.public,
                b"baft-test/v1",
            )
            .await
            .is_err()
        });
        let client = initiate(left, &initiator.private, &responder.public, b"baft-test/v2").await;
        assert!(client.is_err() || server.await.unwrap());
    }
}
