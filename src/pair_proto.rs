//! Pairing code and one-shot pairing exchange (ADR-0050, stage 3).
//!
//! The listener host creates a [`PairCode`] and serves exactly one pairing over TLS 1.3 with a
//! short-lived self-signed certificate. The joiner learns that certificate's SHA-256 pin from the
//! code (so it authenticates the listener) and proves possession of the one-time token (so the
//! listener authenticates the joiner). Messages are length-prefixed JSON with hard bounds and
//! deadlines; the token, CSR key and CA key are never logged. This module only moves public
//! material; the caller supplies the approval/signing closure (ADR-0050 stage 4).
#![allow(dead_code)] // wired into the agent and helper in ADR-0050 stage 4

use std::{
    collections::HashMap,
    future::Future,
    net::{IpAddr, SocketAddr},
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Semaphore, mpsc},
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

pub const CODE_PREFIX: &str = "BAFT1-";
const MAX_CODE_CHARS: usize = 1400;
const MAX_MESSAGE: usize = 16 * 1024;
/// Wrong or unreadable presentations allowed per source address, then that address is ignored.
const MAX_FAILURES_PER_IP: u32 = 3;
/// Total wrong presentations after which the window closes (a guessing attack, not a typo).
const MAX_FAILURES_TOTAL: u32 = 20;
const MAX_CONCURRENT: usize = 16;
/// TLS handshake plus request must complete within this (slow-client protection).
const HELLO_DEADLINE: Duration = Duration::from_secs(5);
const WRITE_DEADLINE: Duration = Duration::from_secs(10);
/// The joiner waits for the listener's decision; must exceed the listener's approval wait.
pub const RESPONSE_DEADLINE: Duration = Duration::from_secs(180);
/// After a successful response the listener waits for the joiner's final acknowledgement.
const ACK_DEADLINE: Duration = Duration::from_secs(60);
/// A pairing code is valid for this long.
pub const CODE_TTL_SECS: u64 = 15 * 60;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairCode {
    pub v: u8,
    /// Listener address the joiner dials.
    pub host: String,
    pub port: u16,
    /// SHA-256 (hex) of the listener's pairing certificate.
    pub pin: String,
    /// 128-bit one-time token, hex. A bearer secret until it expires or is used.
    pub token: String,
    pub tunnel_id: String,
    pub direction: String,
    pub listener_id: String,
    pub joiner_id: String,
    pub service_port: u16,
    pub tunnel_port: u16,
    /// Name the listener's tunnel certificate carries (`DNS:`/`IP:`).
    pub server_name: String,
    /// Unix seconds after which the listener refuses.
    pub exp: u64,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_id(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

impl PairCode {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.v == 1, "unsupported pairing code version");
        ensure!(
            is_hex(&self.pin, 64) && is_hex(&self.token, 32),
            "malformed pairing secret"
        );
        ensure!(
            is_id(&self.tunnel_id) && is_id(&self.listener_id) && is_id(&self.joiner_id),
            "invalid identifier"
        );
        ensure!(self.listener_id != self.joiner_id, "node ids must differ");
        ensure!(
            matches!(self.direction.as_str(), "direct" | "reverse"),
            "invalid direction"
        );
        ensure!(
            self.port >= 1024 && self.service_port >= 1024 && self.tunnel_port >= 1024,
            "invalid port"
        );
        ensure!(
            !self.host.is_empty() && self.host.len() <= 253 && !self.host.contains(['/', ' ', '@']),
            "invalid host"
        );
        ensure!(
            self.server_name.starts_with("DNS:") || self.server_name.starts_with("IP:"),
            "invalid server name"
        );
        Ok(())
    }

    pub fn expired(&self) -> bool {
        now_secs() > self.exp
    }

    pub fn encode(&self) -> Result<String> {
        self.validate()?;
        let json = serde_json::to_vec(self)?;
        Ok(format!("{CODE_PREFIX}{}", URL_SAFE_NO_PAD.encode(json)))
    }

    pub fn decode(text: &str) -> Result<Self> {
        let text = text.trim();
        ensure!(text.len() <= MAX_CODE_CHARS, "pairing code is too long");
        let body = text
            .strip_prefix(CODE_PREFIX)
            .context("not a BAFT pairing code")?;
        let json = URL_SAFE_NO_PAD
            .decode(body)
            .context("pairing code is damaged")?;
        let code: Self = serde_json::from_slice(&json).context("pairing code is damaged")?;
        code.validate()?;
        Ok(code)
    }
}

/// Fresh 128-bit hex token.
pub fn new_token() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Ephemeral self-signed pairing identity. Returns (cert PEM path contents are read back) the
/// TLS acceptor and the SHA-256 pin of its certificate.
pub fn pairing_identity(scratch: &Path) -> Result<(TlsAcceptor, String)> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(scratch)?;
    let key = scratch.join("pair.key");
    let crt = scratch.join("pair.crt");
    crate::pair_ca::run(
        &[
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-nodes",
            "-keyout",
            &key.to_string_lossy(),
            "-out",
            &crt.to_string_lossy(),
            "-days",
            "1",
            "-subj",
            "/CN=baft-pairing",
        ],
        None,
    )?;
    let chain = crate::tls::certificates(&crt)?;
    let pin = hex::encode(Sha256::digest(chain[0].as_ref()));
    let acceptor = crate::tls::pairing_acceptor(&crt, &key)?;
    let _ = std::fs::remove_file(&key);
    let _ = std::fs::remove_file(&crt);
    Ok((acceptor, pin))
}

#[derive(Debug)]
struct PinVerifier {
    pin: [u8; 32],
    schemes: Vec<SignatureScheme>,
    algs: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let got: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        if intermediates.is_empty() && got == self.pin {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "pairing certificate pin mismatch".into(),
            ))
        }
    }
    fn verify_tls12_signature(
        &self,
        _m: &[u8],
        _c: &CertificateDer<'_>,
        _d: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not allowed".into()))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.algs)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes.clone()
    }
}

fn connector(pin_hex: &str) -> Result<TlsConnector> {
    let pin: [u8; 32] = hex::decode(pin_hex)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("bad pin"))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let algs = provider.signature_verification_algorithms;
    let verifier = PinVerifier {
        pin,
        schemes: algs.supported_schemes(),
        algs,
    };
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinRequest {
    pub token: String,
    pub node_id: String,
    pub csr_pem: String,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JoinResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate_pem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_pem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crl_pem: Option<String>,
    /// The listener's public pairing bundle (JSON); trusted because the channel is authenticated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_bundle: Option<String>,
    /// Generic, non-enumerating reason when `ok` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ack {
    pub ok: bool,
}

async fn write_msg<S: AsyncWriteExt + Unpin>(s: &mut S, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX_MESSAGE, "message too large");
    timeout(WRITE_DEADLINE, async {
        s.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
        s.write_all(&bytes).await?;
        s.flush().await
    })
    .await
    .context("write timed out")??;
    Ok(())
}

async fn read_msg<S: AsyncReadExt + Unpin, T: for<'a> Deserialize<'a>>(
    s: &mut S,
    deadline: Duration,
) -> Result<T> {
    timeout(deadline, async {
        let mut len = [0u8; 4];
        s.read_exact(&mut len).await?;
        let n = u32::from_be_bytes(len) as usize;
        ensure!(n > 0 && n <= MAX_MESSAGE, "message size out of bounds");
        let mut buf = vec![0u8; n];
        s.read_exact(&mut buf).await?;
        Ok(serde_json::from_slice(&buf)?)
    })
    .await
    .context("read timed out")?
}

/// What the listener's approval/signing step decided about a verified joiner.
pub type Decision = Result<JoinResponse>;

fn refused() -> JoinResponse {
    JoinResponse {
        ok: false,
        reason: Some("pairing refused".into()),
        ..Default::default()
    }
}

#[derive(Default)]
struct Limits {
    per_ip: HashMap<IpAddr, u32>,
    total: u32,
}

impl Limits {
    fn banned(&self, ip: IpAddr) -> bool {
        self.per_ip.get(&ip).copied().unwrap_or(0) >= MAX_FAILURES_PER_IP
    }
    /// Record one failure; true when the whole window must close.
    fn fail(&mut self, ip: IpAddr) -> bool {
        *self.per_ip.entry(ip).or_default() += 1;
        self.total += 1;
        self.total >= MAX_FAILURES_TOTAL
    }
}

type Outcome = std::result::Result<SocketAddr, String>;

struct Shared<F> {
    code: PairCode,
    acceptor: TlsAcceptor,
    decide: F,
    limits: std::sync::Mutex<Limits>,
    used: std::sync::atomic::AtomicBool,
}

async fn handle<F, Fut>(
    sh: Arc<Shared<F>>,
    tcp: TcpStream,
    peer: SocketAddr,
    tx: mpsc::Sender<Outcome>,
) where
    F: Fn(JoinRequest, SocketAddr) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Decision> + Send,
{
    let fail = |sh: &Shared<F>| sh.limits.lock().is_ok_and(|mut l| l.fail(peer.ip()));
    let hello = timeout(HELLO_DEADLINE, async {
        let mut tls = sh.acceptor.accept(tcp).await?;
        let req: JoinRequest = read_msg(&mut tls, HELLO_DEADLINE).await?;
        anyhow::Ok((tls, req))
    })
    .await;
    let Ok(Ok((mut tls, req))) = hello else {
        if fail(&sh) {
            let _ = tx
                .send(Err("too many failed pairing attempts; window closed".into()))
                .await;
        }
        return;
    };
    let valid =
        ct_eq(&req.token, &sh.code.token) && req.node_id == sh.code.joiner_id && !sh.code.expired();
    if !valid {
        let close = fail(&sh);
        let _ = write_msg(&mut tls, &refused()).await;
        if close {
            let _ = tx
                .send(Err("too many failed pairing attempts; window closed".into()))
                .await;
        }
        return;
    }
    // Single presentation: the first token-valid request ends the window whatever the outcome.
    if sh.used.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let _ = write_msg(&mut tls, &refused()).await;
        return;
    }
    let resp = (sh.decide)(req, peer).await.unwrap_or_else(|_| refused());
    if write_msg(&mut tls, &resp).await.is_err() {
        let _ = tx
            .send(Err("could not deliver the pairing response".into()))
            .await;
        return;
    }
    if !resp.ok {
        let _ = tx.send(Err("pairing was not approved".into())).await;
        return;
    }
    // Commit only after the joiner confirms it installed everything.
    match read_msg::<_, Ack>(&mut tls, ACK_DEADLINE).await {
        Ok(Ack { ok: true }) => {
            let _ = tx.send(Ok(peer)).await;
        }
        _ => {
            let _ = tx
                .send(Err(
                    "the other server did not confirm; pairing cancelled".into()
                ))
                .await;
        }
    }
    let _ = tls.shutdown().await;
}

/// Serve at most one pairing. `decide` runs only for a request whose token and node id match; it
/// asks the user for approval and signs (ADR-0050). Returns the joiner's address only after the
/// joiner acknowledged a complete install; errors on expiry, denial, a missing acknowledgement,
/// or too many failed presentations.
pub async fn serve<F, Fut>(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    code: &PairCode,
    decide: F,
) -> Result<SocketAddr>
where
    F: Fn(JoinRequest, SocketAddr) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Decision> + Send,
{
    let sh = Arc::new(Shared {
        code: code.clone(),
        acceptor,
        decide,
        limits: std::sync::Mutex::new(Limits::default()),
        used: std::sync::atomic::AtomicBool::new(false),
    });
    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT));
    let (tx, mut rx) = mpsc::channel::<Outcome>(8);
    loop {
        let remaining = code.exp.saturating_sub(now_secs());
        ensure!(remaining > 0, "pairing code expired");
        tokio::select! {
            accepted = timeout(Duration::from_secs(remaining), listener.accept()) => {
                let (tcp, peer) = match accepted {
                    Err(_) => bail!("pairing code expired"),
                    Ok(Err(_)) => continue,
                    Ok(Ok(v)) => v,
                };
                let banned = sh.limits.lock().is_ok_and(|l| l.banned(peer.ip()));
                let Ok(permit) = sem.clone().try_acquire_owned() else { continue };
                if banned { continue }
                let (sh2, tx2) = (sh.clone(), tx.clone());
                tokio::spawn(async move {
                    handle(sh2, tcp, peer, tx2).await;
                    drop(permit);
                });
            }
            out = rx.recv() => match out {
                Some(Ok(peer)) => return Ok(peer),
                Some(Err(e)) => bail!(e),
                None => bail!("pairing listener stopped"),
            },
        }
    }
}

/// The joiner's open connection after a successful response; send the final acknowledgement.
pub struct JoinSession {
    tls: tokio_rustls::client::TlsStream<TcpStream>,
}

impl JoinSession {
    pub async fn ack(mut self, ok: bool) -> Result<()> {
        write_msg(&mut self.tls, &Ack { ok }).await?;
        let _ = self.tls.shutdown().await;
        Ok(())
    }
}

/// Dial the listener named in `code`, authenticate it by pin, send the CSR, wait for the decision
/// (as long as the user may take to approve). On success the caller installs the material and
/// then calls [`JoinSession::ack`].
pub async fn join(code: &PairCode, csr_pem: &str) -> Result<(JoinResponse, Option<JoinSession>)> {
    code.validate()?;
    ensure!(!code.expired(), "pairing code expired");
    let tcp = timeout(
        HELLO_DEADLINE * 3,
        TcpStream::connect((code.host.as_str(), code.port)),
    )
    .await
    .context("connect timed out")?
    .context("cannot reach the other server (is its tunnel port open?)")?;
    let name = ServerName::try_from("baft-pairing").expect("static name");
    let mut tls = timeout(HELLO_DEADLINE * 3, connector(&code.pin)?.connect(name, tcp))
        .await
        .context("TLS timed out")?
        .context("the server did not present the expected pairing certificate")?;
    write_msg(
        &mut tls,
        &JoinRequest {
            token: code.token.clone(),
            node_id: code.joiner_id.clone(),
            csr_pem: csr_pem.into(),
        },
    )
    .await?;
    let resp: JoinResponse = read_msg(&mut tls, RESPONSE_DEADLINE).await?;
    let session = resp.ok.then_some(JoinSession { tls });
    Ok((resp, session))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pair_ca::{Member, create, sign};
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pair-proto-{tag}-{:x}", rand::random::<u64>()))
    }

    fn code(port: u16, pin: &str, exp: u64) -> PairCode {
        PairCode {
            v: 1,
            host: "127.0.0.1".into(),
            port,
            pin: pin.into(),
            token: new_token(),
            tunnel_id: "t1".into(),
            direction: "direct".into(),
            listener_id: "node-a".into(),
            joiner_id: "node-b".into(),
            service_port: 9000,
            tunnel_port: 8443,
            server_name: "IP:127.0.0.1".into(),
            exp,
        }
    }

    fn csr(dir: &Path, id: &str) -> String {
        std::fs::create_dir_all(dir).unwrap();
        let key = dir.join(format!("{id}.key"));
        String::from_utf8(
            crate::pair_ca::run(
                &[
                    "req",
                    "-new",
                    "-newkey",
                    "ec",
                    "-pkeyopt",
                    "ec_paramgen_curve:P-256",
                    "-nodes",
                    "-keyout",
                    &key.to_string_lossy(),
                    "-subj",
                    &format!("/CN={id}"),
                ],
                None,
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn members() -> [Member; 2] {
        [
            Member {
                node_id: "node-a".into(),
                san: "IP:127.0.0.1".into(),
            },
            Member {
                node_id: "node-b".into(),
                san: "DNS:node-b.pair.test".into(),
            },
        ]
    }

    #[test]
    fn code_round_trips_and_rejects_garbage() {
        let c = code(9443, &"a".repeat(64), now_secs() + 60);
        let text = c.encode().unwrap();
        assert!(text.starts_with("BAFT1-") && text.len() < MAX_CODE_CHARS);
        assert_eq!(PairCode::decode(&text).unwrap(), c);
        assert!(PairCode::decode("BAFT1-!!!").is_err());
        assert!(PairCode::decode("hello").is_err());
        let mut bad = c.clone();
        bad.token = "zz".into();
        assert!(bad.encode().is_err());
    }

    async fn start(
        work: &Path,
        approve: bool,
        delay: Duration,
    ) -> (
        PairCode,
        tokio::task::JoinHandle<Result<SocketAddr>>,
        PathBuf,
    ) {
        let ca_dir = work.join("ca");
        create(&ca_dir, "Pair CA t", &members()).unwrap();
        let (acceptor, pin) = pairing_identity(&work.join("id")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let c = code(port, &pin, now_secs() + 60);
        let (c2, ca2) = (c.clone(), ca_dir.clone());
        let server = tokio::spawn(async move {
            serve(listener, acceptor, &c2, move |req, _peer| {
                let ca2 = ca2.clone();
                async move {
                    tokio::time::sleep(delay).await;
                    anyhow::ensure!(approve, "denied");
                    let leaf = sign(&ca2, &req.node_id, &req.csr_pem)?;
                    Ok(JoinResponse {
                        ok: true,
                        certificate_pem: Some(leaf),
                        ca_pem: Some(std::fs::read_to_string(ca2.join("ca.crt"))?),
                        crl_pem: Some(std::fs::read_to_string(ca2.join("ca.crl"))?),
                        ..Default::default()
                    })
                }
            })
            .await
        });
        (c, server, ca_dir)
    }

    #[tokio::test]
    async fn pairing_issues_a_valid_certificate_once_and_commits_only_after_ack() {
        let work = tmp("ok");
        let (c, server, _ca) = start(&work, true, Duration::from_millis(0)).await;
        // wrong pin: the joiner refuses before sending anything
        let mut wrong = c.clone();
        wrong.pin = "b".repeat(64);
        assert!(join(&wrong, "x").await.is_err());
        // wrong token is refused generically
        let mut bad = c.clone();
        bad.token = new_token();
        let (r, s) = join(&bad, &csr(&work, "node-b")).await.unwrap();
        assert!(!r.ok && r.certificate_pem.is_none() && s.is_none());
        // correct join
        let (r, session) = join(&c, &csr(&work, "node-b")).await.unwrap();
        assert!(r.ok, "{r:?}");
        std::fs::write(work.join("n.crt"), r.certificate_pem.unwrap()).unwrap();
        std::fs::write(work.join("ca.crt"), r.ca_pem.unwrap()).unwrap();
        std::fs::write(work.join("ca.crl"), r.crl_pem.unwrap()).unwrap();
        crate::tls::verify_local_certificate(
            &work.join("ca.crt"),
            &work.join("ca.crl"),
            &work.join("n.crt"),
            "DNS:node-b.pair.test",
        )
        .unwrap();
        session.unwrap().ack(true).await.unwrap();
        server.await.unwrap().unwrap();
        assert!(join(&c, &csr(&work, "node-b")).await.is_err(), "single use");
        let _ = std::fs::remove_dir_all(&work);
    }

    #[tokio::test]
    async fn missing_ack_or_nack_cancels_the_pairing() {
        let work = tmp("nack");
        let (c, server, _ca) = start(&work, true, Duration::from_millis(0)).await;
        let (r, session) = join(&c, &csr(&work, "node-b")).await.unwrap();
        assert!(r.ok);
        session.unwrap().ack(false).await.unwrap();
        assert!(server.await.unwrap().is_err());
        let _ = std::fs::remove_dir_all(&work);
    }

    #[tokio::test]
    async fn deny_ends_the_window_and_the_token_cannot_be_retried() {
        let work = tmp("deny");
        let (c, server, _ca) = start(&work, false, Duration::from_millis(0)).await;
        let (r, _s) = join(&c, &csr(&work, "node-b")).await.unwrap();
        assert!(!r.ok);
        assert!(server.await.unwrap().is_err());
        assert!(join(&c, &csr(&work, "node-b")).await.is_err());
        let _ = std::fs::remove_dir_all(&work);
    }

    #[tokio::test]
    async fn the_joiner_waits_longer_than_the_old_15_second_limit_for_a_slow_approval() {
        assert!(RESPONSE_DEADLINE > Duration::from_secs(150));
        let work = tmp("slow");
        let (c, server, _ca) = start(&work, true, Duration::from_millis(1500)).await;
        let started = std::time::Instant::now();
        let (r, session) = join(&c, &csr(&work, "node-b")).await.unwrap();
        assert!(r.ok && started.elapsed() >= Duration::from_millis(1400));
        session.unwrap().ack(true).await.unwrap();
        server.await.unwrap().unwrap();
        let _ = std::fs::remove_dir_all(&work);
    }

    #[tokio::test]
    async fn a_stalled_client_does_not_block_pairing_and_one_address_cannot_close_the_window() {
        let work = tmp("dos");
        let (c, server, _ca) = start(&work, true, Duration::from_millis(0)).await;
        // a connection that never speaks
        let _idle = TcpStream::connect(("127.0.0.1", c.port)).await.unwrap();
        // three garbage presentations from this address ban it, but the window stays open
        for _ in 0..3 {
            let mut bad = c.clone();
            bad.token = new_token();
            let _ = join(&bad, "x").await;
        }
        assert!(!server.is_finished(), "window must stay open");
        // the same (now banned) address can no longer pair; the window is closed by the test
        assert!(join(&c, &csr(&work, "node-b")).await.is_err());
        server.abort();
        let _ = std::fs::remove_dir_all(&work);
    }
}
