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
    net::SocketAddr,
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
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

pub const CODE_PREFIX: &str = "BAFT1-";
const MAX_CODE_CHARS: usize = 1400;
const MAX_MESSAGE: usize = 16 * 1024;
const MAX_TOKEN_ATTEMPTS: u32 = 3;
const IO_DEADLINE: Duration = Duration::from_secs(15);
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

fn now() -> u64 {
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
        now() > self.exp
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JoinResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate_pem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_pem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crl_pem: Option<String>,
    /// Generic, non-enumerating reason when `ok` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

async fn write_msg<S: AsyncWriteExt + Unpin>(s: &mut S, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX_MESSAGE, "message too large");
    timeout(IO_DEADLINE, async {
        s.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
        s.write_all(&bytes).await?;
        s.flush().await
    })
    .await
    .context("write timed out")??;
    Ok(())
}

async fn read_msg<S: AsyncReadExt + Unpin, T: for<'a> Deserialize<'a>>(s: &mut S) -> Result<T> {
    timeout(IO_DEADLINE, async {
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

/// Serve at most one successful pairing. `decide` runs only for a request whose token and node id
/// match; it asks the user for approval and signs (ADR-0050). Returns when a pairing was issued,
/// after [`MAX_TOKEN_ATTEMPTS`] wrong tokens, or at the code's expiry.
pub async fn serve<F>(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    code: &PairCode,
    mut decide: F,
) -> Result<SocketAddr>
where
    F: FnMut(&JoinRequest, SocketAddr) -> Decision,
{
    let mut failures = 0u32;
    loop {
        let remaining = code.exp.saturating_sub(now());
        ensure!(remaining > 0, "pairing code expired");
        let (tcp, peer) = match timeout(Duration::from_secs(remaining), listener.accept()).await {
            Ok(r) => r?,
            Err(_) => bail!("pairing code expired"),
        };
        let Ok(Ok(mut tls)) = timeout(IO_DEADLINE, acceptor.accept(tcp)).await else {
            continue;
        };
        let Ok(req) = read_msg::<_, JoinRequest>(&mut tls).await else {
            continue;
        };
        let token_ok = ct_eq(&req.token, &code.token);
        if !(token_ok && req.node_id == code.joiner_id) {
            failures += 1;
            let _ = write_msg(
                &mut tls,
                &JoinResponse {
                    ok: false,
                    certificate_pem: None,
                    ca_pem: None,
                    crl_pem: None,
                    reason: Some("pairing refused".into()),
                },
            )
            .await;
            ensure!(
                failures < MAX_TOKEN_ATTEMPTS,
                "too many wrong pairing attempts; listener closed"
            );
            continue;
        }
        let resp = decide(&req, peer).unwrap_or(JoinResponse {
            ok: false,
            certificate_pem: None,
            ca_pem: None,
            crl_pem: None,
            reason: Some("pairing refused".into()),
        });
        write_msg(&mut tls, &resp).await?;
        let _ = tls.shutdown().await;
        if resp.ok {
            return Ok(peer);
        }
    }
}

/// Dial the listener named in `code`, authenticate it by pin, send the CSR, return the response.
pub async fn join(code: &PairCode, csr_pem: &str) -> Result<JoinResponse> {
    code.validate()?;
    ensure!(!code.expired(), "pairing code expired");
    let tcp = timeout(
        IO_DEADLINE,
        TcpStream::connect((code.host.as_str(), code.port)),
    )
    .await
    .context("connect timed out")?
    .context("cannot reach the other server (is its tunnel port open?)")?;
    let name = ServerName::try_from("baft-pairing").expect("static name");
    let mut tls = timeout(IO_DEADLINE, connector(&code.pin)?.connect(name, tcp))
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
    let resp: JoinResponse = read_msg(&mut tls).await?;
    Ok(resp)
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

    #[test]
    fn code_round_trips_and_rejects_garbage() {
        let c = code(9443, &"a".repeat(64), now() + 60);
        let text = c.encode().unwrap();
        assert!(text.starts_with("BAFT1-") && text.len() < MAX_CODE_CHARS);
        assert_eq!(PairCode::decode(&text).unwrap(), c);
        assert!(PairCode::decode("BAFT1-!!!").is_err());
        assert!(PairCode::decode("hello").is_err());
        let mut bad = c.clone();
        bad.token = "zz".into();
        assert!(bad.encode().is_err());
    }

    #[tokio::test]
    async fn pairing_issues_a_valid_certificate_once() {
        let work = tmp("ok");
        let ca_dir = work.join("ca");
        let members = [
            Member {
                node_id: "node-a".into(),
                san: "IP:127.0.0.1".into(),
            },
            Member {
                node_id: "node-b".into(),
                san: "DNS:node-b.pair.test".into(),
            },
        ];
        let ca_pem = create(&ca_dir, "Pair CA t", &members).unwrap();
        let (acceptor, pin) = pairing_identity(&work.join("id")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let c = code(port, &pin, now() + 60);
        let c2 = c.clone();
        let ca_dir2 = ca_dir.clone();
        let server = tokio::spawn(async move {
            serve(listener, acceptor, &c2, move |req, _peer| {
                let leaf = sign(&ca_dir2, &req.node_id, &req.csr_pem)?;
                Ok(JoinResponse {
                    ok: true,
                    certificate_pem: Some(leaf),
                    ca_pem: Some(std::fs::read_to_string(ca_dir2.join("ca.crt"))?),
                    crl_pem: Some(std::fs::read_to_string(ca_dir2.join("ca.crl"))?),
                    reason: None,
                })
            })
            .await
        });
        // wrong pin: the joiner refuses before sending anything
        let mut wrong = c.clone();
        wrong.pin = "b".repeat(64);
        assert!(join(&wrong, "x").await.is_err());
        // wrong token is refused generically
        let mut bad = c.clone();
        bad.token = new_token();
        let r = join(&bad, &csr(&work, "node-b")).await.unwrap();
        assert!(!r.ok && r.certificate_pem.is_none());
        // correct join
        let r = join(&c, &csr(&work, "node-b")).await.unwrap();
        assert!(r.ok, "{r:?}");
        assert_eq!(r.ca_pem.as_deref(), Some(ca_pem.as_str()));
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
        server.await.unwrap().unwrap();
        // single use: the listener is gone
        assert!(join(&c, &csr(&work, "node-b")).await.is_err());
        let _ = std::fs::remove_dir_all(&work);
    }

    #[tokio::test]
    async fn listener_closes_after_three_wrong_tokens() {
        let work = tmp("brute");
        let (acceptor, pin) = pairing_identity(&work.join("id")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let c = code(port, &pin, now() + 60);
        let c2 = c.clone();
        let server = tokio::spawn(async move {
            serve(listener, acceptor, &c2, |_, _| bail!("must not be called")).await
        });
        for _ in 0..3 {
            let mut bad = c.clone();
            bad.token = new_token();
            let _ = join(&bad, "x").await;
        }
        assert!(server.await.unwrap().is_err());
        let _ = std::fs::remove_dir_all(&work);
    }
}
