//! Fleet hub listener (ADR-0045 F1, read-only): receives `node.status.v1` reports over mutual
//! TLS 1.3 and hands them to the pure rules in `fleet.rs`. Nothing here decides policy; it only
//! turns a verified client certificate into a [`PeerCert`] (node id, serial and EKUs come from the
//! certificate, never the body: F-5), bounds the request (F-9) and maps the rules' answer to an
//! HTTP status. Chain and CRL checks are done by `tls::server_config` (F-8). The listener is
//! default off and has no control action: a request can only add or refresh an inventory row.
#![allow(dead_code)]

use crate::fleet::{self, Hub, PeerCert, Reject};
use anyhow::{Context, Result, bail};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, watch};
use tokio::time::timeout;

/// Report path; the only route.
pub const REPORT_PATH: &str = "/v1/fleet/report";
/// Concurrent connections served at once; the rest are closed unread.
pub const MAX_CONNECTIONS: usize = 32;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_HEAD_BYTES: usize = 4096;

/// Where accepted reports go. Implemented for a bare `Mutex<Hub>` (tests) and for the agent's
/// shared context, so the listener never needs to know about the rest of the agent.
pub trait HubStore: Send + Sync + 'static {
    fn hub(&self) -> &Mutex<Hub>;
}

impl HubStore for Mutex<Hub> {
    fn hub(&self) -> &Mutex<Hub> {
        self
    }
}

/// What the verified client certificate says about the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub node_id: String,
    pub serial: String,
    pub ekus: Vec<String>,
}

/// Read node id (exactly one `baft-fleet://<id>.fleet.baft.invalid` URI SAN, the Fleet CA profile), serial and EKU OIDs from the
/// leaf certificate. `anyExtendedKeyUsage` is refused outright (F-4).
pub fn peer_identity(leaf_der: &[u8]) -> Result<Identity> {
    use x509_parser::extensions::GeneralName;
    let (rest, cert) = x509_parser::parse_x509_certificate(leaf_der)
        .map_err(|_| anyhow::anyhow!("client certificate is not valid X.509 DER"))?;
    if !rest.is_empty() {
        bail!("client certificate has trailing data");
    }
    let san = cert
        .subject_alternative_name()
        .context("parse client certificate subject alternative name")?
        .context("client certificate has no subject alternative name")?;
    let mut node_id: Option<&str> = None;
    for name in &san.value.general_names {
        if let GeneralName::URI(uri) = name {
            if let Some(id) = uri
                .strip_prefix("baft-fleet://")
                .and_then(|r| r.strip_suffix(".fleet.baft.invalid"))
            {
                if node_id.replace(id).is_some() {
                    bail!("client certificate has several node identities");
                }
            }
        }
    }
    let node_id = node_id.context("client certificate has no node identity")?;
    if !(1..=63).contains(&node_id.len())
        || !node_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        bail!("client certificate node id is not [a-z0-9-]{{1,63}}");
    }
    let eku = cert
        .extended_key_usage()
        .context("parse client certificate extended key usage")?
        .context("client certificate has no extended key usage")?
        .value;
    if eku.any {
        bail!("anyExtendedKeyUsage is not accepted on the Fleet channel");
    }
    let ekus = eku.other.iter().map(|o| o.to_id_string()).collect();
    Ok(Identity {
        node_id: node_id.to_owned(),
        serial: cert.raw_serial_as_string(),
        ekus,
    })
}

fn status_of(r: &std::result::Result<(), Reject>) -> u16 {
    match r {
        Ok(()) => 204,
        Err(Reject::Profile) => 403,
        Err(Reject::Malformed(_)) | Err(Reject::UnknownVersion) => 400,
        Err(Reject::TooLarge) => 413,
        Err(Reject::Replay) => 409,
        Err(Reject::TooFast) => 429,
        Err(Reject::InventoryFull) => 507,
    }
}

fn reason(code: u16) -> &'static str {
    match code {
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        _ => "Insufficient Storage",
    }
}

/// Parse one bounded POST and answer it. Returns the status that was (or would have been) sent.
/// No keep-alive, no chunked bodies, no `Expect`, one route.
pub async fn handle<S>(stream: &mut S, id: &Identity, hub: &Mutex<Hub>, now: u64) -> u16
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let code = match read_request(stream).await {
        Ok(body) => {
            let ekus: Vec<&str> = id.ekus.iter().map(String::as_str).collect();
            let peer = PeerCert {
                node_id: &id.node_id,
                serial: &id.serial,
                ekus: &ekus,
            };
            let r = hub
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .accept(&peer, &body, now);
            status_of(&r)
        }
        Err(code) => code,
    };
    let head = format!(
        "HTTP/1.1 {code} {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        reason(code)
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.shutdown().await;
    code
}

async fn read_request<S>(stream: &mut S) -> std::result::Result<Vec<u8>, u16>
where
    S: AsyncRead + Unpin,
{
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
        if buf.len() > MAX_HEAD_BYTES {
            return Err(400);
        }
        let n = stream.read(&mut chunk).await.map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| 400u16)?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, path, version) = (first.next(), first.next(), first.next());
    if version != Some("HTTP/1.1") || first.next().is_some() {
        return Err(400);
    }
    if path != Some(REPORT_PATH) {
        return Err(404);
    }
    if method != Some("POST") {
        return Err(405);
    }
    let mut length: Option<usize> = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(400u16)?;
        // HTTP/1.1 forbids whitespace between the field name and the colon; accepting
        // `Content-Length : 5` would let a proxy and this listener disagree on the framing.
        if name.is_empty() || name.bytes().any(|b| b == b' ' || b == b'\t') {
            return Err(400);
        }
        match name.to_ascii_lowercase().as_str() {
            "content-length" => {
                if length.is_some() {
                    return Err(400);
                }
                // `str::trim` would hide a bare CR/LF and `usize::from_str` accepts a leading
                // sign; HTTP defines a length as ASCII digits with only SP/HTAB around it.
                let value = value.trim_matches([' ', '\t']);
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(400);
                }
                length = Some(value.parse::<usize>().map_err(|_| 400u16)?);
            }
            "transfer-encoding" | "expect" | "content-encoding" => return Err(400),
            _ => {}
        }
    }
    let length = length.ok_or(411u16)?;
    if length > fleet::MAX_REPORT_BYTES {
        return Err(413);
    }
    let mut body = buf[head_end + 4..].to_vec();
    if body.len() > length {
        return Err(400);
    }
    while body.len() < length {
        let want = (length - body.len()).min(chunk.len());
        let n = stream.read(&mut chunk[..want]).await.map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(body)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Accept loop. Every connection must complete a TLS 1.3 handshake with a client certificate
/// that chains to the pinned CA and is not revoked; the profile check happens in the rules.
pub async fn serve<S: HubStore>(
    listener: TcpListener,
    acceptor: Arc<crate::tls::ReloadingAcceptor>,
    hub: Arc<S>,
    mut shutdown: watch::Receiver<bool>,
) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        tokio::select! {
            _ = shutdown.changed() => return,
            accepted = listener.accept() => {
                let Ok((tcp, _)) = accepted else { continue };
                let Ok(permit) = slots.clone().try_acquire_owned() else { continue };
                let (acceptor, hub) = (acceptor.clone(), hub.clone());
                tokio::spawn(async move {
                    let _permit = permit;
                    let Ok(tls_acceptor) = acceptor.current() else { return };
                    let Ok(Ok(mut tls)) = timeout(HANDSHAKE_TIMEOUT, tls_acceptor.accept(tcp)).await else {
                        return;
                    };
                    let id = tls
                        .get_ref()
                        .1
                        .peer_certificates()
                        .and_then(|c| c.first())
                        .and_then(|leaf| peer_identity(leaf.as_ref()).ok());
                    let Some(id) = id else { return };
                    let _ = timeout(REQUEST_TIMEOUT, handle(&mut tls, &id, hub.hub(), unix_now())).await;
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::{EKU_FLEET_NODE, EKU_TUNNEL};
    use rcgen::{
        CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
        KeyUsagePurpose, SanType, string::Ia5String,
    };
    use std::path::{Path, PathBuf};
    use tokio::io::duplex;

    fn oid(s: &str) -> ExtendedKeyUsagePurpose {
        ExtendedKeyUsagePurpose::Other(s.split('.').map(|p| p.parse().unwrap()).collect())
    }

    /// A client certificate with exactly the given private EKUs (plus clientAuth, which the
    /// TLS verifier needs) and the given node URI.
    fn client_cert(
        dir: &Path,
        name: &str,
        node: &str,
        ekus: &[&str],
        issuer: &rcgen::Issuer<'static, KeyPair>,
    ) -> (PathBuf, PathBuf) {
        let mut p = CertificateParams::new(Vec::new()).unwrap();
        p.distinguished_name = DistinguishedName::new();
        p.distinguished_name.push(DnType::CommonName, node);
        p.is_ca = IsCa::ExplicitNoCa;
        p.key_usages.push(KeyUsagePurpose::DigitalSignature);
        let mut usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        usages.extend(ekus.iter().map(|e| oid(e)));
        p.extended_key_usages = usages;
        p.use_authority_key_identifier_extension = true;
        p.subject_alt_names.push(SanType::URI(
            Ia5String::try_from(format!("baft-fleet://{node}.fleet.baft.invalid")).unwrap(),
        ));
        let key = KeyPair::generate().unwrap();
        let cert = p.signed_by(&key, issuer).unwrap();
        let (c, k) = (
            dir.join(format!("{name}.pem")),
            dir.join(format!("{name}.key")),
        );
        std::fs::write(&c, cert.pem()).unwrap();
        std::fs::write(&k, key.serialize_pem()).unwrap();
        (c, k)
    }

    fn der_of(cert_pem: &Path) -> Vec<u8> {
        crate::tls::certificates(cert_pem).unwrap()[0]
            .as_ref()
            .to_vec()
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "baft-fleet-hub-{tag}-{}-{}",
            std::process::id(),
            unix_now()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn identity_comes_from_the_certificate_and_refuses_any_eku() {
        let d = tmp("id");
        let ca = crate::tls::tests::make_ca(&d, "ca");
        let (c, _) = client_cert(&d, "n1", "node-a", &[EKU_FLEET_NODE], &ca.issuer);
        let id = peer_identity(&der_of(&c)).unwrap();
        assert_eq!(id.node_id, "node-a");
        assert_eq!(id.ekus, vec![EKU_FLEET_NODE.to_owned()]);
        assert!(!id.serial.is_empty());
        let (c, _) = client_cert(&d, "bad", "node-a", &[], &ca.issuer);
        // No private EKU at all: identity parses, the rules then refuse the profile.
        assert!(peer_identity(&der_of(&c)).unwrap().ekus.is_empty());
        assert!(peer_identity(b"not a certificate").is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    fn ident(ekus: &[&str]) -> Identity {
        Identity {
            node_id: "node-a".into(),
            serial: "01".into(),
            ekus: ekus.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    async fn post(raw: Vec<u8>, id: &Identity, hub: &Mutex<Hub>, now: u64) -> u16 {
        let (mut client, mut server) = duplex(64 * 1024);
        let w = tokio::spawn(async move {
            client.write_all(&raw).await.unwrap();
            let _ = client.shutdown().await;
            let mut out = Vec::new();
            let _ = client.read_to_end(&mut out).await;
            out
        });
        let code = handle(&mut server, id, hub, now).await;
        drop(server);
        let out = w.await.unwrap();
        assert!(String::from_utf8_lossy(&out).starts_with(&format!("HTTP/1.1 {code} ")));
        code
    }

    fn req(body: &str) -> Vec<u8> {
        format!(
            "POST {REPORT_PATH} HTTP/1.1\r\nHost: h\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[tokio::test]
    async fn http_layer_maps_rules_and_bounds_requests() {
        let hub = Mutex::new(Hub::default());
        let good = ident(&[EKU_FLEET_NODE]);
        let body = r#"{"schema":"node.status.v1","seq":1,"instances":[{"name":"t","health":"healthy","rtt_ms":12.5}]}"#;
        assert_eq!(post(req(body), &good, &hub, 100).await, 204);
        assert_eq!(hub.lock().unwrap().nodes().count(), 1);
        // Replay of the same seq.
        assert_eq!(post(req(body), &good, &hub, 200).await, 409);
        // A tunnel certificate is never a fleet node (F-2).
        assert_eq!(post(req(body), &ident(&[EKU_TUNNEL]), &hub, 300).await, 403);
        assert_eq!(post(req(body), &ident(&[]), &hub, 300).await, 403);
        // Bounds and framing.
        let big = "x".repeat(fleet::MAX_REPORT_BYTES + 1);
        assert_eq!(post(req(&big), &good, &hub, 400).await, 413);
        assert_eq!(post(req("{"), &good, &hub, 400).await, 400);
        let get = format!("GET {REPORT_PATH} HTTP/1.1\r\n\r\n").into_bytes();
        assert_eq!(post(get, &good, &hub, 400).await, 405);
        let other = b"POST /v1/other HTTP/1.1\r\nContent-Length: 0\r\n\r\n".to_vec();
        assert_eq!(post(other, &good, &hub, 400).await, 404);
        let chunked = format!("POST {REPORT_PATH} HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n")
            .into_bytes();
        assert_eq!(post(chunked, &good, &hub, 400).await, 400);
        let nolen = format!("POST {REPORT_PATH} HTTP/1.1\r\n\r\n").into_bytes();
        assert_eq!(post(nolen, &good, &hub, 400).await, 411);
        let dup = format!(
            "POST {REPORT_PATH} HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n{{"
        )
        .into_bytes();
        assert_eq!(post(dup, &good, &hub, 400).await, 400);
        // Truncated body.
        let short =
            format!("POST {REPORT_PATH} HTTP/1.1\r\nContent-Length: 50\r\n\r\n{{").into_bytes();
        assert_eq!(post(short, &good, &hub, 400).await, 400);
    }

    #[tokio::test]
    async fn content_length_must_be_plain_digits_without_header_whitespace() {
        let hub = Mutex::new(Hub::default());
        let good = ident(&[EKU_FLEET_NODE]);
        let body = r#"{"schema":"node.status.v1","seq":1,"instances":[]}"#;
        for head in [
            // A leading sign is not a length.
            format!(
                "POST {REPORT_PATH} HTTP/1.1\r\nHost: h\r\nContent-Length: +{}\r\n\r\n{body}",
                body.len()
            ),
            // A bare LF must not be trimmed into a valid value.
            format!(
                "POST {REPORT_PATH} HTTP/1.1\r\nHost: h\r\nContent-Length: {}\n\r\n\r\n{body}",
                body.len()
            ),
            // Whitespace between the field name and the colon is not a header.
            format!(
                "POST {REPORT_PATH} HTTP/1.1\r\nHost: h\r\nContent-Length : {}\r\n\r\n{body}",
                body.len()
            ),
            // Whitespace inside the value is not a length.
            format!(
                "POST {REPORT_PATH} HTTP/1.1\r\nHost: h\r\nContent-Length: {} {}\r\n\r\n{body}",
                body.len(),
                body.len()
            ),
        ] {
            assert_eq!(post(head.into_bytes(), &good, &hub, 100).await, 400, "{head:?}");
        }
        // Legal SP/HTAB padding around the digits is still accepted.
        let padded = format!(
            "POST {REPORT_PATH} HTTP/1.1\r\nHost: h\r\nContent-Length:\t{} \r\n\r\n{body}",
            body.len()
        );
        assert_eq!(post(padded.into_bytes(), &good, &hub, 200).await, 204);
        assert_eq!(hub.lock().unwrap().nodes().count(), 1);
    }

    #[tokio::test]
    async fn real_mtls_listener_accepts_fleet_node_and_rejects_others() {
        let d = tmp("mtls");
        let ca = crate::tls::tests::make_ca(&d, "ca");
        let (hub_cert, hub_key) =
            crate::tls::tests::leaf(&d, "hub", "urn:baft-tunnel:node:hub", &ca.issuer);
        let other_ca = crate::tls::tests::make_ca(&d, "other");
        let acceptor = Arc::new(
            crate::tls::ReloadingAcceptor::new(&ca.ca_pem, &ca.crl_pem, &hub_cert, &hub_key)
                .unwrap(),
        );
        let hub = Arc::new(Mutex::new(Hub::default()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, rx) = watch::channel(false);
        let server = tokio::spawn(serve(listener, acceptor, hub.clone(), rx));

        let send = |cert: PathBuf, key: PathBuf, seq: u64| {
            let ca_pem = ca.ca_pem.clone();
            let crl = ca.crl_pem.clone();
            async move {
                let cfg = crate::tls::client_config(&ca_pem, &crl, &cert, &key).unwrap();
                let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
                let mut tls = crate::tls::connector(cfg)
                    .connect(crate::tls::server_name("localhost").unwrap(), tcp)
                    .await
                    .ok()?;
                let body = format!(r#"{{"schema":"node.status.v1","seq":{seq},"instances":[]}}"#);
                tls.write_all(&req(&body)).await.ok()?;
                let mut out = Vec::new();
                let _ = tls.read_to_end(&mut out).await;
                String::from_utf8_lossy(&out)
                    .split(' ')
                    .nth(1)
                    .map(str::to_owned)
            }
        };

        let (c, k) = client_cert(&d, "n", "node-a", &[EKU_FLEET_NODE], &ca.issuer);
        assert_eq!(send(c.clone(), k.clone(), 1).await.as_deref(), Some("204"));
        assert_eq!(hub.lock().unwrap().nodes().count(), 1);
        // Same cert, same seq: replay.
        assert_eq!(send(c, k, 1).await.as_deref(), Some("409"));
        // A tunnel-profile certificate passes TLS but has no Fleet identity: no response (F-2).
        let (tc, tk) =
            crate::tls::tests::leaf(&d, "tun", "urn:baft-tunnel:node:node-b", &ca.issuer);
        let got = send(tc, tk, 1).await;
        assert!(
            got.is_none() || got.as_deref() == Some(""),
            "unexpected {got:?}"
        );
        // A fleet-shaped certificate that carries only the tunnel EKU is refused by the rules.
        let (tc, tk) = client_cert(&d, "tunfleet", "node-d", &[EKU_TUNNEL], &ca.issuer);
        assert_eq!(send(tc, tk, 1).await.as_deref(), Some("403"));
        // A certificate from another CA never gets a response.
        let (oc, ok) = client_cert(&d, "evil", "node-c", &[EKU_FLEET_NODE], &other_ca.issuer);
        let got = send(oc, ok, 1).await;
        assert!(
            got.is_none() || got.as_deref() == Some(""),
            "unexpected {got:?}"
        );
        assert_eq!(hub.lock().unwrap().nodes().count(), 1);

        let _ = stop.send(true);
        let _ = server.await;
        let _ = std::fs::remove_dir_all(d);
    }
}
