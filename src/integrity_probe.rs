//! Bounded protocol-v2 integrity probe exchange (ADR-0028).
//!
//! The wire format is fixed-size and length-prefixed. Payload bytes are streamed through a
//! small stack buffer; no peer-controlled length is used for allocation.

use std::{
    future::Future,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot},
    time::timeout,
};

pub const PROBE_BYTES: usize = 64 * 1024;
pub const PROBE_DEADLINE: Duration = Duration::from_secs(10);
pub const PROBE_COOLDOWN: Duration = Duration::from_secs(60);
const NONCE_BYTES: usize = 32;
const CHUNK_BYTES: usize = 4096;
const REQUEST_FRAME_BYTES: u32 = (NONCE_BYTES + PROBE_BYTES) as u32;
const RESPONSE_FRAME_BYTES: u32 = (2 + NONCE_BYTES + 4 + 32) as u32;
const PROTOCOL_VERSION: u16 = 2;
const REQUEST_MAGIC: &[u8; 4] = b"BTP2";
const RESPONSE_MAGIC: &[u8; 4] = b"BTR2";

/// A typed probe failure. A result is classified from this kind, never from message text.
#[allow(dead_code)] // Stable public classification surface; some variants are platform-gated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeFailure {
    IntegrityMismatch,
    InvalidResponse,
    RateLimited,
    Busy,
    PeerUnavailable,
    UnsupportedProtocol,
    Timeout,
}

#[allow(dead_code)] // Stable classification helpers are consumed by platform-specific control code.
impl ProbeFailure {
    /// The stable reason code exposed on the local control socket.
    pub fn reason(self) -> &'static str {
        match self {
            Self::IntegrityMismatch => "integrity_mismatch",
            Self::InvalidResponse => "invalid_probe_response",
            Self::RateLimited => "rate_limited",
            Self::Busy => "probe_busy",
            Self::PeerUnavailable => "peer_unavailable",
            Self::UnsupportedProtocol => "unsupported_protocol",
            Self::Timeout => "timeout",
        }
    }

    /// Only evidence that the probe itself was wrong is a `fail`; limits and transport are `unknown`.
    pub fn result(self) -> &'static str {
        match self {
            Self::IntegrityMismatch | Self::InvalidResponse => "fail",
            _ => "unknown",
        }
    }
}

impl std::fmt::Display for ProbeFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::IntegrityMismatch => "integrity probe payload or digest did not match",
            Self::InvalidResponse => "integrity probe response was malformed",
            Self::RateLimited => "integrity probe rate limit: one start per 60 seconds",
            Self::Busy => "integrity probe already active for this tunnel instance",
            Self::PeerUnavailable => "integrity probe peer or authenticated session is unavailable",
            Self::UnsupportedProtocol => "peer does not support integrity probe protocol v2",
            Self::Timeout => "integrity probe timed out",
        })
    }
}

impl std::error::Error for ProbeFailure {}

/// Keep a human-readable context while staying downcastable to the typed kind.
fn failure(kind: ProbeFailure, context: &'static str) -> anyhow::Error {
    anyhow::Error::new(kind).context(context)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeResult {
    pub bytes: usize,
    pub duration_ms: u128,
}

pub struct ProbeRequest {
    pub reply: oneshot::Sender<std::result::Result<ProbeResult, String>>,
}

struct ProbeControlInner {
    next_id: AtomicU64,
    current: StdMutex<Option<(u64, mpsc::Sender<ProbeRequest>)>>,
}

#[derive(Clone)]
pub struct ProbeControl {
    inner: Arc<ProbeControlInner>,
}

impl Default for ProbeControl {
    fn default() -> Self {
        Self {
            inner: Arc::new(ProbeControlInner {
                next_id: AtomicU64::new(1),
                current: StdMutex::new(None),
            }),
        }
    }
}

pub struct ProbeSessionGuard {
    inner: Arc<ProbeControlInner>,
    id: u64,
}

impl Drop for ProbeSessionGuard {
    fn drop(&mut self) {
        let mut current = self.inner.current.lock().unwrap_or_else(|e| e.into_inner());
        if current.as_ref().is_some_and(|(id, _)| *id == self.id) {
            *current = None;
        }
    }
}

impl ProbeControl {
    /// Bind one active authenticated yamux session as the target for local probe requests.
    pub fn begin_session(&self) -> (ProbeSessionGuard, mpsc::Receiver<ProbeRequest>) {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel(1);
        *self.inner.current.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, sender));
        (
            ProbeSessionGuard {
                inner: self.inner.clone(),
                id,
            },
            receiver,
        )
    }

    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.inner
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// Submit only to a currently active authenticated session; never queue across reconnects.
    #[cfg(any(target_os = "linux", test))]
    pub async fn request(&self) -> Result<ProbeResult> {
        let sender = self
            .inner
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|(_, sender)| sender.clone())
            .ok_or_else(|| {
                failure(
                    ProbeFailure::PeerUnavailable,
                    "authenticated tunnel session is unavailable",
                )
            })?;
        let (reply, response) = oneshot::channel();
        sender.try_send(ProbeRequest { reply }).map_err(|_| {
            failure(
                ProbeFailure::Busy,
                "integrity probe request is busy or session is closing",
            )
        })?;
        timeout(PROBE_DEADLINE, response)
            .await
            .map_err(|_| failure(ProbeFailure::Timeout, "integrity probe request timed out"))?
            .map_err(|_| {
                failure(
                    ProbeFailure::PeerUnavailable,
                    "integrity probe session ended",
                )
            })?
            .map_err(|_| {
                failure(
                    ProbeFailure::PeerUnavailable,
                    "integrity probe peer reported a failure",
                )
            })
    }
}

pub struct ProbeLimiter {
    active: Arc<Semaphore>,
    last_started: Mutex<Option<Instant>>,
}

impl Default for ProbeLimiter {
    fn default() -> Self {
        Self {
            active: Arc::new(Semaphore::new(1)),
            last_started: Mutex::new(None),
        }
    }
}

impl ProbeLimiter {
    /// Atomically enforces one active probe and one start per cooldown window.
    pub async fn acquire(&self) -> Result<OwnedSemaphorePermit> {
        let permit = self.active.clone().try_acquire_owned().map_err(|_| {
            failure(
                ProbeFailure::Busy,
                "integrity probe already active for this tunnel instance",
            )
        })?;
        let mut last = self.last_started.lock().await;
        if last.is_some_and(|at| at.elapsed() < PROBE_COOLDOWN) {
            drop(last);
            drop(permit);
            return Err(failure(
                ProbeFailure::RateLimited,
                "integrity probe rate limit: one start per 60 seconds",
            ));
        }
        *last = Some(Instant::now());
        Ok(permit)
    }
}

/// Run stream opening, authorization and the transfer under one end-to-end deadline.
pub async fn run_on_session<S, F, Fut>(open_and_authorize: F) -> Result<ProbeResult>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<S>>,
{
    run_on_session_with_deadline(PROBE_DEADLINE, open_and_authorize).await
}

async fn run_on_session_with_deadline<S, F, Fut>(
    deadline: Duration,
    open_and_authorize: F,
) -> Result<ProbeResult>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<S>>,
{
    timeout(deadline, async {
        let started = Instant::now();
        let mut stream = open_and_authorize().await?;
        let mut result = run_initiator_inner(&mut stream).await?;
        result.duration_ms = started.elapsed().as_millis();
        Ok(result)
    })
    .await
    .map_err(|_| {
        failure(
            ProbeFailure::Timeout,
            "integrity probe exceeded its 10-second end-to-end deadline",
        )
    })?
}

/// Initiate one fresh 64 KiB challenge and verify the authenticated peer's response.
#[cfg(test)]
async fn run_initiator<S>(stream: &mut S) -> Result<ProbeResult>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    timeout(PROBE_DEADLINE, run_initiator_inner(stream))
        .await
        .map_err(|_| {
            failure(
                ProbeFailure::Timeout,
                "integrity probe exceeded its 10-second end-to-end deadline",
            )
        })?
}

async fn run_initiator_inner<S>(stream: &mut S) -> Result<ProbeResult>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let started = Instant::now();
    let mut nonce = [0u8; NONCE_BYTES];
    let mut payload = vec![0u8; PROBE_BYTES];
    OsRng.fill_bytes(&mut nonce);
    OsRng.fill_bytes(&mut payload);
    let expected_digest = Sha256::digest(&payload);

    stream.write_all(REQUEST_MAGIC).await?;
    stream.write_all(&REQUEST_FRAME_BYTES.to_be_bytes()).await?;
    stream.write_all(&nonce).await?;
    for chunk in payload.chunks(CHUNK_BYTES) {
        stream.write_all(chunk).await?;
    }
    stream.flush().await?;
    // Half-close the request direction; the response remains readable.
    stream.shutdown().await?;

    let mut magic = [0u8; 4];
    stream
        .read_exact(&mut magic)
        .await
        .context("read probe response magic")?;
    if &magic != RESPONSE_MAGIC {
        return Err(failure(
            ProbeFailure::InvalidResponse,
            "malformed integrity probe response magic",
        ));
    }
    let mut length = [0u8; 4];
    stream.read_exact(&mut length).await?;
    if u32::from_be_bytes(length) != RESPONSE_FRAME_BYTES {
        return Err(failure(
            ProbeFailure::InvalidResponse,
            "invalid integrity probe response frame length",
        ));
    }
    let mut version = [0u8; 2];
    let mut response_nonce = [0u8; NONCE_BYTES];
    let mut count = [0u8; 4];
    let mut digest = [0u8; 32];
    stream.read_exact(&mut version).await?;
    stream.read_exact(&mut response_nonce).await?;
    stream.read_exact(&mut count).await?;
    stream.read_exact(&mut digest).await?;
    if u16::from_be_bytes(version) != PROTOCOL_VERSION {
        return Err(failure(
            ProbeFailure::UnsupportedProtocol,
            "unsupported integrity probe response version",
        ));
    }
    if response_nonce != nonce {
        return Err(failure(
            ProbeFailure::IntegrityMismatch,
            "integrity probe response nonce mismatch (replay or wrong session)",
        ));
    }
    if u32::from_be_bytes(count) as usize != PROBE_BYTES {
        return Err(failure(
            ProbeFailure::IntegrityMismatch,
            "integrity probe response byte count mismatch",
        ));
    }
    if digest != expected_digest.as_slice() {
        return Err(failure(
            ProbeFailure::IntegrityMismatch,
            "integrity probe SHA-256 mismatch",
        ));
    }
    let mut extra = [0u8; 1];
    if stream.read(&mut extra).await? != 0 {
        return Err(failure(
            ProbeFailure::InvalidResponse,
            "integrity probe response contains trailing bytes",
        ));
    }
    Ok(ProbeResult {
        bytes: PROBE_BYTES,
        duration_ms: started.elapsed().as_millis(),
    })
}

/// Respond to exactly one authenticated IntegrityProbe stream. This never touches a TCP target.
pub async fn respond<S>(stream: &mut S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    timeout(PROBE_DEADLINE, respond_inner(stream))
        .await
        .map_err(|_| {
            failure(
                ProbeFailure::Timeout,
                "integrity probe responder exceeded its 10-second deadline",
            )
        })?
}

async fn respond_inner<S>(stream: &mut S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut magic = [0u8; 4];
    stream
        .read_exact(&mut magic)
        .await
        .context("read probe request magic")?;
    if &magic != REQUEST_MAGIC {
        return Err(failure(
            ProbeFailure::InvalidResponse,
            "malformed integrity probe request magic",
        ));
    }
    let mut length = [0u8; 4];
    stream.read_exact(&mut length).await?;
    if u32::from_be_bytes(length) != REQUEST_FRAME_BYTES {
        return Err(failure(
            ProbeFailure::InvalidResponse,
            "invalid integrity probe request frame length",
        ));
    }
    let mut nonce = [0u8; NONCE_BYTES];
    stream.read_exact(&mut nonce).await?;
    let mut hasher = Sha256::new();
    let mut remaining = PROBE_BYTES;
    let mut chunk = [0u8; CHUNK_BYTES];
    while remaining > 0 {
        let take = remaining.min(chunk.len());
        stream.read_exact(&mut chunk[..take]).await?;
        hasher.update(&chunk[..take]);
        remaining -= take;
    }
    let mut extra = [0u8; 1];
    if stream.read(&mut extra).await? != 0 {
        return Err(failure(
            ProbeFailure::InvalidResponse,
            "integrity probe request contains trailing payload bytes",
        ));
    }
    let digest = hasher.finalize();

    stream.write_all(RESPONSE_MAGIC).await?;
    stream
        .write_all(&RESPONSE_FRAME_BYTES.to_be_bytes())
        .await?;
    stream.write_all(&PROTOCOL_VERSION.to_be_bytes()).await?;
    stream.write_all(&nonce).await?;
    stream
        .write_all(&(PROBE_BYTES as u32).to_be_bytes())
        .await?;
    stream.write_all(&digest).await?;
    stream.flush().await?;
    stream.shutdown().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    async fn relay<R, W>(
        reader: &mut R,
        writer: &mut W,
        flip_at: Option<usize>,
    ) -> std::io::Result<()>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut offset = 0usize;
        let mut buffer = [0u8; 8192];
        loop {
            let count = reader.read(&mut buffer).await?;
            if count == 0 {
                writer.shutdown().await?;
                return Ok(());
            }
            if let Some(index) = flip_at {
                if (offset..offset + count).contains(&index) {
                    buffer[index - offset] ^= 0x01;
                }
            }
            writer.write_all(&buffer[..count]).await?;
            offset += count;
        }
    }

    #[tokio::test]
    async fn initiator_rejects_a_tampered_payload_on_the_integrity_path() {
        let (mut initiator, client_side) = duplex(PROBE_BYTES + 1024);
        let (peer_side, mut responder) = duplex(PROBE_BYTES + 1024);
        let responder_task = tokio::spawn(async move { respond(&mut responder).await });
        let relay_task = tokio::spawn(async move {
            let (mut client_read, mut client_write) = tokio::io::split(client_side);
            let (mut peer_read, mut peer_write) = tokio::io::split(peer_side);
            let (forward, reverse) = tokio::join!(
                relay(&mut client_read, &mut peer_write, Some(4 + 4 + NONCE_BYTES),),
                relay(&mut peer_read, &mut client_write, None),
            );
            forward?;
            reverse?;
            Ok::<(), std::io::Error>(())
        });

        let error = run_initiator(&mut initiator).await.unwrap_err();
        assert!(
            format!("{error:#}").contains("SHA-256 mismatch"),
            "tampered payload was not rejected by the integrity check: {error:#}"
        );
        relay_task
            .await
            .expect("tamper relay task panicked")
            .unwrap();
        responder_task
            .await
            .expect("integrity responder task panicked")
            .unwrap();
    }

    #[tokio::test]
    async fn fixed_size_probe_round_trip_verifies_digest_and_nonce() {
        let (mut initiator, mut responder) = duplex(PROBE_BYTES + 1024);
        let task = tokio::spawn(async move { respond(&mut responder).await });
        let result = run_initiator(&mut initiator).await.unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(result.bytes, PROBE_BYTES);
        assert!(result.duration_ms <= 10_000);
    }

    #[tokio::test]
    async fn session_deadline_includes_stream_open_and_authorization() {
        let result = run_on_session_with_deadline(Duration::from_millis(10), || async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let (stream, _peer) = duplex(16);
            Ok(stream)
        })
        .await;
        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("end-to-end deadline"));
    }

    #[tokio::test]
    async fn responder_rejects_oversized_frame_before_reading_payload() {
        let (mut initiator, mut responder) = duplex(128);
        let task = tokio::spawn(async move { respond(&mut responder).await });
        initiator.write_all(REQUEST_MAGIC).await.unwrap();
        initiator
            .write_all(&(REQUEST_FRAME_BYTES + 1).to_be_bytes())
            .await
            .unwrap();
        assert!(task.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn responder_rejects_truncated_payload() {
        let (mut initiator, mut responder) = duplex(128);
        let task = tokio::spawn(async move { respond(&mut responder).await });
        initiator.write_all(REQUEST_MAGIC).await.unwrap();
        initiator
            .write_all(&REQUEST_FRAME_BYTES.to_be_bytes())
            .await
            .unwrap();
        initiator.write_all(&[0u8; NONCE_BYTES]).await.unwrap();
        initiator.shutdown().await.unwrap();
        assert!(task.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn control_rejects_requests_without_an_active_authenticated_session() {
        let control = ProbeControl::default();
        assert!(control.request().await.is_err());
        let (guard, mut receiver) = control.begin_session();
        let request = tokio::spawn({
            let control = control.clone();
            async move { control.request().await }
        });
        let command = receiver.recv().await.unwrap();
        let _ = command.reply.send(Err("test-unavailable".into()));
        assert!(request.await.unwrap().is_err());
        drop(guard);
        assert!(control.request().await.is_err());
    }

    #[tokio::test]
    async fn limiter_enforces_single_active_and_cooldown() {
        let limiter = ProbeLimiter::default();
        let permit = limiter.acquire().await.unwrap();
        assert!(limiter.acquire().await.is_err());
        drop(permit);
        assert!(limiter.acquire().await.is_err());
    }
}
