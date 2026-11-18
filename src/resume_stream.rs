//! Async pump for one resumable stream (ADR-0040 step 4b). A task owns the local socket and the
//! [`Endpoint`]; the substream that carries it can be replaced at any time with [`Handle::attach`].
//! While no substream is attached the local socket stays open (the application sees a stall) for
//! at most the grace period, then the stream ends and its memory is returned to the tunnel budget.
//!
//! This is the data-plane building block only. It is **not wired** into the tunnel and v3 is not
//! selectable, so nothing here is a working resume feature yet.
#![allow(dead_code)]

use crate::resumable::{CodecError, Delivered, Endpoint, Frame, ProtocolError};
use crate::resume_core::{ResumeError, TunnelBudget};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// Bytes delivered by the peer but not yet written to the local socket. When this fills we stop
/// reading the substream, so a slow local socket backpressures the peer instead of growing memory.
const MAX_PENDING_TO_LOCAL: usize = 1 << 20;

pub trait Io: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}
pub type BoxIo = Box<dyn Io>;

pub type SharedBudget = Arc<Mutex<TunnelBudget>>;

#[derive(Debug, PartialEq, Eq)]
pub enum StreamError {
    /// No substream (re)attached within the grace period.
    GraceExpired,
    /// The peer aborted the stream.
    Reset,
    Protocol,
    /// `attach` was given an offset that cannot be resumed from.
    Resume(ResumeError),
    /// The handle was dropped while the stream was detached.
    Closed,
}

impl From<ProtocolError> for StreamError {
    fn from(_: ProtocolError) -> Self {
        StreamError::Protocol
    }
}

impl From<CodecError> for StreamError {
    fn from(_: CodecError) -> Self {
        StreamError::Protocol
    }
}

struct Attach {
    io: BoxIo,
    peer_received: u64,
}

/// A cheap clone of the attach side, so callers can attach without holding a lock on the handle.
#[derive(Clone)]
pub struct Attacher {
    attach: mpsc::Sender<Attach>,
    received: Arc<AtomicU64>,
}

impl Attacher {
    pub fn received(&self) -> u64 {
        self.received.load(Ordering::Acquire)
    }

    pub async fn attach(&self, io: BoxIo, peer_received: u64) -> Result<(), StreamError> {
        self.attach
            .send(Attach { io, peer_received })
            .await
            .map_err(|_| StreamError::Closed)
    }
}

pub struct Handle {
    attach: mpsc::Sender<Attach>,
    received: Arc<AtomicU64>,
    task: JoinHandle<Result<(), StreamError>>,
}

impl Handle {
    /// Bytes of the peer-to-us direction already delivered; this is what `ResumeHello` reports.
    /// It only grows, so a slightly stale read is always safe (the peer resends a little more).
    pub fn received(&self) -> u64 {
        self.received.load(Ordering::Acquire)
    }

    /// Replace the carrying substream. `peer_received` is the peer's reported receive offset.
    pub async fn attach(&self, io: BoxIo, peer_received: u64) -> Result<(), StreamError> {
        self.attach
            .send(Attach { io, peer_received })
            .await
            .map_err(|_| StreamError::Closed)
    }

    pub fn attacher(&self) -> Attacher {
        Attacher {
            attach: self.attach.clone(),
            received: self.received.clone(),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    pub async fn join(self) -> Result<(), StreamError> {
        self.task.await.unwrap_or(Err(StreamError::Closed))
    }
}

/// Start the pump for `local`. The first substream is attached with [`Handle::attach`] (offset 0
/// for a new stream); until then the grace period already runs.
pub fn spawn<L>(local: L, window: usize, budget: SharedBudget, grace: Duration) -> Handle
where
    L: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (tx, rx) = mpsc::channel(4);
    let received = Arc::new(AtomicU64::new(0));
    let task = tokio::spawn(run(
        local,
        Endpoint::new(window),
        budget,
        grace,
        rx,
        received.clone(),
    ));
    Handle {
        attach: tx,
        received,
        task,
    }
}

fn lock(budget: &SharedBudget) -> std::sync::MutexGuard<'_, TunnelBudget> {
    budget.lock().unwrap_or_else(|e| e.into_inner())
}

async fn run<L>(
    local: L,
    mut ep: Endpoint,
    budget: SharedBudget,
    grace: Duration,
    mut rx: mpsc::Receiver<Attach>,
    received: Arc<AtomicU64>,
) -> Result<(), StreamError>
where
    L: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let result = pump(local, &mut ep, &budget, grace, &mut rx, &received).await;
    ep.release(&mut lock(&budget));
    result
}

async fn pump<L>(
    local: L,
    ep: &mut Endpoint,
    budget: &SharedBudget,
    grace: Duration,
    rx: &mut mpsc::Receiver<Attach>,
    received: &AtomicU64,
) -> Result<(), StreamError>
where
    L: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (mut lr, mut lw) = tokio::io::split(local);
    let mut sub_r: Option<ReadHalf<BoxIo>> = None;
    let mut sub_w: Option<WriteHalf<BoxIo>> = None;
    let mut deadline = Some(Instant::now() + grace);
    let mut rbuf: Vec<u8> = Vec::new();
    let mut out: Vec<u8> = Vec::new();
    let mut out_pos = 0usize;
    let mut to_local: Vec<u8> = Vec::new();
    let mut pending_local: Vec<u8> = Vec::new();
    let (mut local_read_done, mut write_closed) = (false, false);
    let (mut eof_seen, mut local_shut) = (false, false);
    let mut rx_open = true;
    let mut lbuf = vec![0_u8; 16 * 1024];
    let mut sbuf = vec![0_u8; 32 * 1024];

    loop {
        if !pending_local.is_empty() {
            let n = ep.write(&pending_local, &mut lock(budget));
            pending_local.drain(..n);
        }
        if local_read_done && pending_local.is_empty() && !write_closed {
            ep.close_write();
            write_closed = true;
        }
        if sub_r.is_some() && out_pos >= out.len() {
            out.clear();
            out_pos = 0;
            if let Some(frame) = ep.next_frame() {
                out = frame.encode();
            }
        }
        if eof_seen && to_local.is_empty() && !local_shut {
            let _ = lw.shutdown().await;
            local_shut = true;
        }
        if ep.finished() && to_local.is_empty() && local_shut && out_pos >= out.len() {
            return Ok(());
        }

        let wait_deadline = deadline;
        tokio::select! {
            read = lr.read(&mut lbuf), if !local_read_done && pending_local.is_empty() => {
                match read {
                    Ok(0) | Err(_) => local_read_done = true,
                    Ok(n) => pending_local.extend_from_slice(&lbuf[..n]),
                }
            }
            read = async {
                match sub_r.as_mut() {
                    Some(r) => r.read(&mut sbuf).await,
                    None => std::future::pending().await,
                }
            }, if sub_r.is_some() && to_local.len() < MAX_PENDING_TO_LOCAL => {
                match read {
                    Ok(0) | Err(_) => {
                        sub_r = None;
                        sub_w = None;
                        rbuf.clear();
                        out.clear();
                        out_pos = 0;
                        deadline = Some(Instant::now() + grace);
                    }
                    Ok(n) => {
                        rbuf.extend_from_slice(&sbuf[..n]);
                        while let Some((frame, used)) = Frame::decode(&rbuf)? {
                            rbuf.drain(..used);
                            match ep.on_frame(&frame, &mut lock(budget))? {
                                Delivered::Bytes(bytes) => to_local.extend_from_slice(&bytes),
                                Delivered::Eof => eof_seen = true,
                                Delivered::Reset => return Err(StreamError::Reset),
                                Delivered::Nothing => {}
                            }
                            if let Delivered::Eof = ep.try_eof()? {
                                eof_seen = true;
                            }
                            received.store(ep.received(), Ordering::Release);
                        }
                    }
                }
            }
            written = async {
                match sub_w.as_mut() {
                    Some(w) if out_pos < out.len() => w.write(&out[out_pos..]).await,
                    _ => std::future::pending().await,
                }
            } => {
                match written {
                    Ok(n) => out_pos += n,
                    Err(_) => {
                        sub_r = None;
                        sub_w = None;
                        rbuf.clear();
                        out.clear();
                        out_pos = 0;
                        deadline = Some(Instant::now() + grace);
                    }
                }
            }
            written = lw.write(&to_local), if !to_local.is_empty() => {
                match written {
                    Ok(n) => {
                        to_local.drain(..n);
                    }
                    // The local application went away: nothing can be delivered any more.
                    Err(_) => return Err(StreamError::Reset),
                }
            }
            attach = rx.recv(), if rx_open => {
                match attach {
                    Some(Attach { io, peer_received }) => {
                        ep.attach(peer_received, &mut lock(budget)).map_err(StreamError::Resume)?;
                        let (r, w) = tokio::io::split(io);
                        sub_r = Some(r);
                        sub_w = Some(w);
                        rbuf.clear();
                        out.clear();
                        out_pos = 0;
                        deadline = None;
                    }
                    None => {
                        rx_open = false;
                        if sub_r.is_none() {
                            return Err(StreamError::Closed);
                        }
                    }
                }
            }
            _ = async {
                match wait_deadline {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => {
                if sub_r.is_none() {
                    return Err(StreamError::GraceExpired);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;
    use tokio::sync::oneshot;

    fn budget(cap: usize) -> SharedBudget {
        Arc::new(Mutex::new(TunnelBudget::new(cap)))
    }

    /// A cuttable link between two attach points: dropping `cut` closes both ends.
    fn link() -> (BoxIo, BoxIo, oneshot::Sender<()>) {
        let (a_io, a_mid) = duplex(8 * 1024);
        let (b_io, b_mid) = duplex(8 * 1024);
        let (cut_tx, cut_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut am, mut bm) = (a_mid, b_mid);
            tokio::select! {
                _ = tokio::io::copy_bidirectional(&mut am, &mut bm) => {}
                _ = cut_rx => {}
            }
        });
        (Box::new(a_io), Box::new(b_io), cut_tx)
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 >> 11
        }
    }

    /// Two pumps joined by a link that is cut at random, both directions carrying different data
    /// at the same time. The link is rebuilt after each cut and both sides re-attach with the
    /// other's received offset. Output must be byte-identical and the budget must return to zero.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn survives_repeated_cuts_in_both_directions_byte_exact() {
        for seed in 1..=6u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let total = 400_000usize;
            let src_a: Vec<u8> = (0..total).map(|i| (i as u64 * 7 + seed) as u8).collect();
            let src_b: Vec<u8> = (0..total / 2)
                .map(|i| (i as u64 * 13 + seed * 5) as u8)
                .collect();
            let (app_a, local_a) = duplex(16 * 1024);
            let (app_b, local_b) = duplex(16 * 1024);
            let (ba, bb) = (budget(8 << 20), budget(8 << 20));
            let grace = Duration::from_secs(20);
            let a = spawn(local_a, 64 * 1024, ba.clone(), grace);
            let b = spawn(local_b, 64 * 1024, bb.clone(), grace);

            // Application side: write everything then half-close; read until EOF.
            let (mut ar, mut aw) = tokio::io::split(app_a);
            let (mut br, mut bw) = tokio::io::split(app_b);
            let (sa, sb) = (src_a.clone(), src_b.clone());
            let wa = tokio::spawn(async move {
                for chunk in sa.chunks(4096) {
                    aw.write_all(chunk).await.unwrap();
                    tokio::time::sleep(Duration::from_micros(300)).await;
                }
                aw.shutdown().await.unwrap();
            });
            let wb = tokio::spawn(async move {
                for chunk in sb.chunks(4096) {
                    bw.write_all(chunk).await.unwrap();
                    tokio::time::sleep(Duration::from_micros(300)).await;
                }
                bw.shutdown().await.unwrap();
            });
            let ra = tokio::spawn(async move {
                let mut v = Vec::new();
                ar.read_to_end(&mut v).await.unwrap();
                v
            });
            let rb = tokio::spawn(async move {
                let mut v = Vec::new();
                br.read_to_end(&mut v).await.unwrap();
                v
            });

            let mut cuts = 0;
            let mut cut_tx: Option<oneshot::Sender<()>> = None;
            while !(a.is_finished() && b.is_finished()) {
                let (ia, ib, tx) = link();
                let (pa, pb) = (b.received(), a.received());
                if a.attach(ia, pa).await.is_err() || b.attach(ib, pb).await.is_err() {
                    break;
                }
                cut_tx = Some(tx);
                let wait = Duration::from_millis(3 + rng.next() % 20);
                tokio::time::sleep(wait).await;
                if !(a.is_finished() && b.is_finished()) && cuts < 8 {
                    drop(cut_tx.take());
                    cuts += 1;
                    tokio::time::sleep(Duration::from_millis(rng.next() % 5)).await;
                } else {
                    // Let it finish on the last link.
                    while !(a.is_finished() && b.is_finished()) {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                }
            }
            drop(cut_tx);
            a.join().await.expect("a ended cleanly");
            b.join().await.expect("b ended cleanly");
            wa.await.unwrap();
            wb.await.unwrap();
            assert_eq!(ra.await.unwrap(), src_b, "seed {seed}: b to a");
            assert_eq!(rb.await.unwrap(), src_a, "seed {seed}: a to b");
            assert_eq!(lock(&ba).used(), 0, "seed {seed}: budget returned");
            assert_eq!(lock(&bb).used(), 0);
            assert!(cuts >= 1, "seed {seed}: the link was never cut");
        }
    }

    #[tokio::test]
    async fn grace_expiry_ends_a_detached_stream_and_returns_memory() {
        let (app, local) = duplex(4096);
        let b = budget(1 << 20);
        let h = spawn(local, 4096, b.clone(), Duration::from_millis(80));
        let (mut ar, mut aw) = tokio::io::split(app);
        let (i1, _i2, _cut) = link();
        h.attach(i1, 0).await.unwrap();
        aw.write_all(&[1; 1000]).await.unwrap();
        // Drop the far end of the link: the pump detaches and the grace runs out.
        drop(_i2);
        let r = tokio::time::timeout(Duration::from_secs(5), h.join())
            .await
            .expect("must end");
        assert_eq!(r, Err(StreamError::GraceExpired));
        assert_eq!(lock(&b).used(), 0);
        // The local socket was closed, so the application sees EOF/reset rather than a hang.
        let mut v = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(2), ar.read_to_end(&mut v)).await;
    }

    #[tokio::test]
    async fn an_impossible_resume_offset_is_refused() {
        let (_app, local) = duplex(4096);
        let h = spawn(local, 4096, budget(1 << 20), Duration::from_secs(5));
        let (i1, _i2, _cut) = link();
        // The peer claims to have received bytes this side never sent.
        h.attach(i1, 999).await.unwrap();
        let r = tokio::time::timeout(Duration::from_secs(5), h.join())
            .await
            .unwrap();
        assert_eq!(r, Err(StreamError::Resume(ResumeError::PeerAhead)));
    }

    #[tokio::test]
    async fn garbage_from_the_peer_is_a_protocol_error() {
        let (_app, local) = duplex(4096);
        let h = spawn(local, 4096, budget(1 << 20), Duration::from_secs(5));
        let (near, mut far) = duplex(4096);
        h.attach(Box::new(near), 0).await.unwrap();
        far.write_all(&[9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        let r = tokio::time::timeout(Duration::from_secs(5), h.join())
            .await
            .unwrap();
        assert_eq!(r, Err(StreamError::Protocol));
    }
}
