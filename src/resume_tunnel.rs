//! Session glue for resumable streams (ADR-0040 step 4c). Direct only and **not reachable from any
//! config or CLI**: nothing in the running tunnel calls this module yet, so it is a tested building
//! block, not a feature.
//!
//! Wire shape on top of an authenticated protocol-v3 yamux session. Every substream starts with
//! the normal stream-open authorization (`TcpForward`), then one kind byte:
//!   0x01 NEW    `u64 stream_id`                      opener starts a resumable stream
//!   0x02 HELLO  `u32 len` + `ResumeHello` JSON       opener asks to resume its streams
//!   0x03 ATTACH `u64 stream_id`                      opener re-attaches one stream after HELLO
//! The acceptor answers HELLO with `u32 len` + `ResumeReply` JSON. The acceptor never answers a
//! failed redeem: it closes the substream, and the opener treats the missing reply as a failure.
//! The caller supplies the *verified* identities of the new session (review M3); this module never
//! reads identities from peer-controlled text.
#![allow(dead_code)]

use crate::authorization::{ConnectionDirection, PROTOCOL_V3, StreamCapability};
use crate::handshake::{self, ResumeGrant};
use crate::resume_registry::Registry;
use crate::resume_stream::{Handle, SharedBudget, spawn};
use crate::resume_wire::{
    Identity, MAX_WIRE_BYTES, ResumeHello, ResumeReply, StreamAnswer, StreamPos,
};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_yamux::{Control, Session};

const KIND_NEW: u8 = 1;
const KIND_HELLO: u8 = 2;
const KIND_ATTACH: u8 = 3;
const STEP_TIMEOUT: Duration = Duration::from_secs(10);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Identities of an authorized session, owned so they can move into tasks.
#[derive(Clone, Debug)]
pub struct Who {
    pub client_id: String,
    pub server_id: String,
    pub tunnel_id: String,
}

impl Who {
    fn identity(&self) -> Identity<'_> {
        Identity {
            client_id: &self.client_id,
            server_id: &self.server_id,
            tunnel_id: &self.tunnel_id,
            direction: ConnectionDirection::Direct,
        }
    }
}

async fn write_json<W: tokio::io::AsyncWrite + Unpin>(w: &mut W, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_WIRE_BYTES {
        bail!("resume message size out of range");
    }
    w.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    w.write_all(bytes).await?;
    w.flush().await?;
    Ok(())
}

/// Resume messages have their own bound, independent of the 2 KiB handshake reader (review M5).
async fn read_json<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> Result<Vec<u8>> {
    let mut len = [0_u8; 4];
    r.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;
    if len == 0 || len > MAX_WIRE_BYTES {
        bail!("resume message length out of range");
    }
    let mut buf = vec![0_u8; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

pub struct Limits {
    pub window: usize,
    pub grace: Duration,
    pub max_sessions: usize,
}

impl Limits {
    /// ADR-0040 defaults: 1 MiB per stream, 60 s grace, a handful of live sessions per tunnel.
    pub fn preview() -> Self {
        Self {
            window: crate::resume_core::DEFAULT_STREAM_WINDOW,
            grace: crate::resume_core::DEFAULT_RESUME_GRACE,
            max_sessions: 16,
        }
    }
}

/// A fresh per-tunnel memory budget (ADR-0040: 64 MiB).
pub fn new_budget() -> SharedBudget {
    Arc::new(Mutex::new(crate::resume_core::TunnelBudget::new(
        crate::resume_core::DEFAULT_TUNNEL_BUDGET,
    )))
}

// ---------------------------------------------------------------- acceptor (service side)

pub struct ServerResume {
    dest: SocketAddr,
    budget: SharedBudget,
    window: usize,
    grace: Duration,
    epoch: Instant,
    registry: Mutex<Registry>,
    streams: Mutex<HashMap<u64, Handle>>,
    offsets: Mutex<HashMap<u64, u64>>,
}

impl ServerResume {
    pub fn new(dest: SocketAddr, budget: SharedBudget, limits: &Limits) -> Arc<Self> {
        Arc::new(Self {
            dest,
            budget,
            window: limits.window,
            grace: limits.grace,
            epoch: Instant::now(),
            registry: Mutex::new(Registry::new(limits.grace, limits.max_sessions)),
            streams: Mutex::new(HashMap::new()),
            offsets: Mutex::new(HashMap::new()),
        })
    }

    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }

    pub fn live_streams(&self) -> usize {
        self.reap();
        lock(&self.streams).len()
    }

    /// Forget finished streams and release their registry slots.
    fn reap(&self) {
        let mut streams = lock(&self.streams);
        let done: Vec<u64> = streams
            .iter()
            .filter(|(_, h)| h.is_finished())
            .map(|(id, _)| *id)
            .collect();
        let mut reg = lock(&self.registry);
        for id in done {
            streams.remove(&id);
            reg.unbind_stream(id);
        }
    }

    /// Serve one authenticated v3 session until it ends, then mark it dropped so its streams can
    /// be resumed (the grace starts now, review H1).
    pub async fn serve_session<S>(
        self: Arc<Self>,
        mut session: Session<S>,
        grant: ResumeGrant,
        who: Who,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let sid = match lock(&self.registry).register(grant.token, grant.binding, self.now()) {
            Ok(sid) => sid,
            Err(error) => {
                eprintln!("resume: session not registered: {error:?}");
                return;
            }
        };
        let who = Arc::new(who);
        loop {
            let item = tokio::select! {
                item = futures::StreamExt::next(&mut session) => item,
                _ = async {
                    // A dropped sender is not a shutdown request.
                    if shutdown.changed().await.is_err() {
                        std::future::pending::<()>().await;
                    }
                } => break,
            };
            let Some(item) = item else { break };
            match item {
                Ok(io) => {
                    let this = self.clone();
                    let who = who.clone();
                    tokio::spawn(async move {
                        if let Err(error) = this.handle_substream(sid, io, who).await {
                            eprintln!("resume substream closed: {error:#}");
                        }
                    });
                }
                Err(_) => break,
            }
        }
        lock(&self.registry).drop_session(sid, self.now());
    }

    async fn handle_substream<S>(&self, sid: u64, mut io: S, who: Arc<Who>) -> Result<()>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let capability = timeout(
            STEP_TIMEOUT,
            handshake::server_stream_open_capability(
                &mut io,
                &who.tunnel_id,
                ConnectionDirection::Direct,
                PROTOCOL_V3,
            ),
        )
        .await
        .context("stream authorization timeout")??;
        if capability != StreamCapability::TcpForward {
            bail!("resume substreams carry TcpForward only");
        }
        let kind = timeout(STEP_TIMEOUT, io.read_u8())
            .await
            .context("kind timeout")??;
        match kind {
            KIND_NEW => {
                let id = timeout(STEP_TIMEOUT, io.read_u64())
                    .await
                    .context("id timeout")??;
                lock(&self.registry)
                    .bind_stream(sid, id)
                    .map_err(|e| anyhow::anyhow!("bind stream: {e:?}"))?;
                let upstream = match timeout(STEP_TIMEOUT, TcpStream::connect(self.dest)).await {
                    Ok(Ok(u)) => u,
                    _ => {
                        lock(&self.registry).unbind_stream(id);
                        bail!("destination connect failed");
                    }
                };
                let handle = spawn(upstream, self.window, self.budget.clone(), self.grace);
                let attacher = handle.attacher();
                lock(&self.streams).insert(id, handle);
                attacher
                    .attach(Box::new(io), 0)
                    .await
                    .map_err(|e| anyhow::anyhow!("attach: {e:?}"))?;
            }
            KIND_HELLO => {
                let hello = ResumeHello::decode(&timeout(STEP_TIMEOUT, read_json(&mut io)).await??)
                    .map_err(|e| anyhow::anyhow!("hello: {e:?}"))?;
                self.reap();
                let outcome = {
                    let mut reg = lock(&self.registry);
                    hello
                        .redeem(&mut reg, sid, &who.identity(), self.now())
                        .map_err(|e| anyhow::anyhow!("redeem: {e:?}"))?
                };
                let mut answers = Vec::new();
                {
                    let mut streams = lock(&self.streams);
                    let mut offsets = lock(&self.offsets);
                    for pos in &outcome.resumable {
                        match streams.get(&pos.stream_id) {
                            Some(h) => {
                                offsets.insert(pos.stream_id, pos.recv_offset);
                                answers.push(StreamAnswer::Resume {
                                    stream_id: pos.stream_id,
                                    recv_offset: h.received(),
                                });
                            }
                            None => answers.push(StreamAnswer::Gone {
                                stream_id: pos.stream_id,
                            }),
                        }
                    }
                    for id in &outcome.gone {
                        answers.push(StreamAnswer::Gone { stream_id: *id });
                    }
                    for id in &outcome.orphaned {
                        streams.remove(id);
                    }
                }
                write_json(
                    &mut io,
                    &ResumeReply { answers }
                        .encode()
                        .map_err(|e| anyhow::anyhow!("reply: {e:?}"))?,
                )
                .await?;
            }
            KIND_ATTACH => {
                let id = timeout(STEP_TIMEOUT, io.read_u64())
                    .await
                    .context("id timeout")??;
                if lock(&self.registry).owner_of(id) != Some(sid) {
                    bail!("attach for a stream this session does not own");
                }
                let offset = lock(&self.offsets)
                    .remove(&id)
                    .context("attach without a resume hello")?;
                let attacher = lock(&self.streams)
                    .get(&id)
                    .map(|h| h.attacher())
                    .context("unknown stream")?;
                attacher
                    .attach(Box::new(io), offset)
                    .await
                    .map_err(|e| anyhow::anyhow!("attach: {e:?}"))?;
            }
            other => bail!("unknown resume substream kind {other}"),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- opener (forwarding side)

pub struct ClientResume {
    tunnel: String,
    budget: SharedBudget,
    window: usize,
    grace: Duration,
    next_id: AtomicU64,
    streams: Mutex<HashMap<u64, Handle>>,
    grant: Mutex<Option<ResumeGrant>>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Resumed {
    pub resumed: Vec<u64>,
    pub gone: Vec<u64>,
}

impl ClientResume {
    pub fn new(tunnel: &str, budget: SharedBudget, limits: &Limits) -> Arc<Self> {
        Arc::new(Self {
            tunnel: tunnel.to_owned(),
            budget,
            window: limits.window,
            grace: limits.grace,
            next_id: AtomicU64::new(1),
            streams: Mutex::new(HashMap::new()),
            grant: Mutex::new(None),
        })
    }

    /// Record the grant of the session the streams currently live on.
    pub fn set_grant(&self, grant: ResumeGrant) {
        *lock(&self.grant) = Some(grant);
    }

    pub fn live_streams(&self) -> usize {
        let mut streams = lock(&self.streams);
        streams.retain(|_, h| !h.is_finished());
        streams.len()
    }

    async fn open_substream(
        &self,
        control: &mut Control,
        kind: u8,
    ) -> Result<tokio_yamux::StreamHandle> {
        let mut io = timeout(STEP_TIMEOUT, control.open_stream())
            .await
            .context("open substream timeout")?
            .context("open substream")?;
        handshake::client_stream_open_versioned(
            &mut io,
            &self.tunnel,
            ConnectionDirection::Direct,
            StreamCapability::TcpForward,
            PROTOCOL_V3,
        )
        .await?;
        io.write_u8(kind).await?;
        Ok(io)
    }

    /// Start a resumable stream carrying `local`.
    pub async fn open_stream<L>(&self, control: &mut Control, local: L) -> Result<u64>
    where
        L: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut io = self.open_substream(control, KIND_NEW).await?;
        io.write_u64(id).await?;
        io.flush().await?;
        let handle = spawn(local, self.window, self.budget.clone(), self.grace);
        let attacher = handle.attacher();
        lock(&self.streams).insert(id, handle);
        attacher
            .attach(Box::new(io), 0)
            .await
            .map_err(|e| anyhow::anyhow!("attach: {e:?}"))?;
        Ok(id)
    }

    /// Resume every live stream on a new, already authenticated session. `new_grant` becomes the
    /// grant the streams live on afterwards. A failed hello leaves the streams detached (their own
    /// grace timers keep running) and the caller may retry on another session.
    pub async fn resume(&self, control: &mut Control, new_grant: ResumeGrant) -> Result<Resumed> {
        let old = lock(&self.grant)
            .clone()
            .context("no previous session grant to resume from")?;
        let asked: Vec<(u64, u64)> = {
            let mut streams = lock(&self.streams);
            streams.retain(|_, h| !h.is_finished());
            streams.iter().map(|(id, h)| (*id, h.received())).collect()
        };
        let hello = ResumeHello {
            old_token: hex::encode(old.token),
            old_client_nonce: old.client_nonce.clone(),
            old_server_nonce: old.server_nonce.clone(),
            streams: asked
                .iter()
                .map(|(stream_id, recv_offset)| StreamPos {
                    stream_id: *stream_id,
                    recv_offset: *recv_offset,
                })
                .collect(),
        };
        let mut io = self.open_substream(control, KIND_HELLO).await?;
        write_json(
            &mut io,
            &hello
                .encode()
                .map_err(|e| anyhow::anyhow!("hello: {e:?}"))?,
        )
        .await?;
        let reply = timeout(STEP_TIMEOUT, read_json(&mut io))
            .await
            .context("no resume reply")?
            .context("resume refused")?;
        let reply = ResumeReply::decode(&reply).map_err(|e| anyhow::anyhow!("reply: {e:?}"))?;
        reply
            .check_matches(&hello.streams)
            .map_err(|e| anyhow::anyhow!("reply does not match request: {e:?}"))?;
        drop(io);
        let mut out = Resumed::default();
        for answer in reply.answers {
            match answer {
                StreamAnswer::Gone { stream_id } => {
                    lock(&self.streams).remove(&stream_id);
                    out.gone.push(stream_id);
                }
                StreamAnswer::Resume {
                    stream_id,
                    recv_offset,
                } => {
                    let attacher = lock(&self.streams).get(&stream_id).map(|h| h.attacher());
                    let Some(attacher) = attacher else {
                        out.gone.push(stream_id);
                        continue;
                    };
                    let mut sub = self.open_substream(control, KIND_ATTACH).await?;
                    sub.write_u64(stream_id).await?;
                    sub.flush().await?;
                    attacher
                        .attach(Box::new(sub), recv_offset)
                        .await
                        .map_err(|e| anyhow::anyhow!("attach: {e:?}"))?;
                    out.resumed.push(stream_id);
                }
            }
        }
        self.set_grant(new_grant);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resume_core::TunnelBudget;
    use tokio::io::duplex;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    const D: ConnectionDirection = ConnectionDirection::Direct;

    fn limits() -> Limits {
        Limits {
            window: 64 * 1024,
            grace: Duration::from_secs(20),
            max_sessions: 8,
        }
    }

    async fn echo_server() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let (mut r, mut w) = s.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                    let _ = w.shutdown().await;
                });
            }
        });
        addr
    }

    /// One authenticated v3 session over a link that can be cut. Returns the client's control and
    /// grant, the server task, and the cut switch.
    async fn session(
        server: &Arc<ServerResume>,
        who: &Who,
    ) -> (
        Control,
        ResumeGrant,
        tokio::task::JoinHandle<()>,
        oneshot::Sender<()>,
    ) {
        let (c_io, c_mid) = duplex(16 * 1024);
        let (s_io, s_mid) = duplex(16 * 1024);
        let (cut_tx, cut_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut a, mut b) = (c_mid, s_mid);
            tokio::select! {
                _ = tokio::io::copy_bidirectional(&mut a, &mut b) => {}
                _ = cut_rx => {}
            }
        });
        let (mut c_io, mut s_io) = (c_io, s_io);
        let (sid, cid, tun) = (
            who.server_id.clone(),
            who.client_id.clone(),
            who.tunnel_id.clone(),
        );
        let (sid2, cid2, tun2) = (sid.clone(), cid.clone(), tun.clone());
        let server_hs = tokio::spawn(async move {
            let g = handshake::server_handshake_grant(&mut s_io, &sid2, &cid2, &tun2, D)
                .await
                .unwrap();
            (g, s_io)
        });
        let client_grant = handshake::client_handshake_grant(&mut c_io, &cid, &sid, &tun, D)
            .await
            .unwrap();
        let (server_grant, s_io) = server_hs.await.unwrap();
        let cfg = crate::tunnel::yamux_config(16);
        let server_session = Session::new_server(s_io, cfg);
        let mut client_session = Session::new_client(c_io, cfg);
        let control = client_session.control();
        tokio::spawn(async move {
            while futures::StreamExt::next(&mut client_session)
                .await
                .is_some()
            {}
        });
        let task = tokio::spawn(server.clone().serve_session(
            server_session,
            server_grant,
            who.clone(),
            tokio::sync::watch::channel(false).1,
        ));
        (control, client_grant, task, cut_tx)
    }

    fn who() -> Who {
        Who {
            client_id: "iran".into(),
            server_id: "abroad".into(),
            tunnel_id: "t".into(),
        }
    }

    /// The whole path: real v3 handshakes, real yamux sessions, a real TCP destination. The link
    /// is cut twice mid-transfer; each time a new session resumes the stream and the echoed bytes
    /// match what was sent.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stream_survives_two_session_cuts_through_real_handshake_and_yamux() {
        let dest = echo_server().await;
        let sb = Arc::new(Mutex::new(TunnelBudget::new(8 << 20)));
        let cb = Arc::new(Mutex::new(TunnelBudget::new(8 << 20)));
        let server = ServerResume::new(dest, sb.clone(), &limits());
        let client = ClientResume::new("t", cb.clone(), &limits());
        let who = who();

        let total = 300_000usize;
        let src: Vec<u8> = (0..total).map(|i| (i as u64 * 31 + 7) as u8).collect();
        let (app, local) = duplex(16 * 1024);
        let (mut ar, mut aw) = tokio::io::split(app);
        let sent = src.clone();
        let writer = tokio::spawn(async move {
            for chunk in sent.chunks(4096) {
                aw.write_all(chunk).await.unwrap();
                tokio::time::sleep(Duration::from_micros(900)).await;
            }
            aw.shutdown().await.unwrap();
        });
        let reader = tokio::spawn(async move {
            let mut v = Vec::new();
            ar.read_to_end(&mut v).await.unwrap();
            v
        });

        let (mut control, grant, mut server_task, mut cut) = session(&server, &who).await;
        client.set_grant(grant);
        client.open_stream(&mut control, local).await.unwrap();

        for round in 0..2 {
            tokio::time::sleep(Duration::from_millis(25)).await;
            drop(cut);
            // The acceptor must have seen the old session end before the opener can redeem (H2).
            timeout(Duration::from_secs(5), &mut server_task)
                .await
                .expect("old session ends")
                .unwrap();
            let (mut c2, g2, t2, cut2) = session(&server, &who).await;
            let resumed = client
                .resume(&mut c2, g2)
                .await
                .unwrap_or_else(|e| panic!("round {round}: {e:#}"));
            assert_eq!(resumed.resumed, vec![1], "round {round}");
            control = c2;
            server_task = t2;
            cut = cut2;
        }
        let echoed = timeout(Duration::from_secs(20), reader)
            .await
            .expect("transfer completes")
            .unwrap();
        writer.await.unwrap();
        assert_eq!(echoed.len(), src.len());
        assert_eq!(echoed, src, "echoed bytes equal sent bytes after two cuts");
        // Both pumps end the stream themselves: the application closed its side, so each direction
        // sends Fin and waits for the peer's Ack. The carrier must stay up while that exchange
        // happens — dropping it first detaches the substream and holds the stream for the whole
        // 20 s grace period, which is a transport-loss path, not the clean teardown this test
        // asserts. Wait for the exchange (bounded), then tear the carrier down and re-assert that
        // nothing lingers.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while server.live_streams() != 0 || client.live_streams() != 0 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the stream did not finish on the live carrier within 5 s: server_live={}, \
                 client_live={}",
                server.live_streams(),
                client.live_streams()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        drop(cut);
        drop(control);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(server.live_streams(), 0);
        assert_eq!(client.live_streams(), 0);
        assert_eq!(sb.lock().unwrap().used(), 0);
        assert_eq!(cb.lock().unwrap().used(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn resume_is_refused_while_the_old_session_is_alive_and_with_a_wrong_identity() {
        let dest = echo_server().await;
        let server = ServerResume::new(
            dest,
            Arc::new(Mutex::new(TunnelBudget::new(1 << 20))),
            &limits(),
        );
        let client = ClientResume::new(
            "t",
            Arc::new(Mutex::new(TunnelBudget::new(1 << 20))),
            &limits(),
        );
        let (mut control, grant, server_task, cut) = session(&server, &who()).await;
        client.set_grant(grant);
        let (_app, local) = duplex(4096);
        client.open_stream(&mut control, local).await.unwrap();
        // A second session while the first is still alive cannot take the stream over.
        let (mut c2, g2, t2, _cut2) = session(&server, &who()).await;
        assert!(
            client.resume(&mut c2, g2).await.is_err(),
            "old session still alive"
        );
        // The failed attempt did not burn the token: after the drop it works.
        drop(cut);
        timeout(Duration::from_secs(5), server_task)
            .await
            .unwrap()
            .unwrap();
        let (mut c3, g3, _t3, _cut3) = session(&server, &who()).await;
        assert_eq!(client.resume(&mut c3, g3).await.unwrap().resumed, vec![1]);
        drop(t2);

        // A server that sees different identities for the new session refuses the same token.
        let server2 = ServerResume::new(
            dest,
            Arc::new(Mutex::new(TunnelBudget::new(1 << 20))),
            &limits(),
        );
        let client2 = ClientResume::new(
            "t",
            Arc::new(Mutex::new(TunnelBudget::new(1 << 20))),
            &limits(),
        );
        let (mut ctl, g, task, cut) = session(&server2, &who()).await;
        client2.set_grant(g);
        let (_app2, local2) = duplex(4096);
        client2.open_stream(&mut ctl, local2).await.unwrap();
        drop(cut);
        timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        let mut other = who();
        other.client_id = "mallory".into();
        let (mut c4, g4, _t4, _cut4) = session(&server2, &other).await;
        assert!(
            client2.resume(&mut c4, g4).await.is_err(),
            "different identity must be refused"
        );
    }
}
