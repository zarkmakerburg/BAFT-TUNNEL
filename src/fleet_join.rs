//! Fleet Join TLS transport primitives (ADR-0051).
//!
//! This module is intentionally separate from tunnel pairing and the persistent Fleet Report
//! listener. Join authenticates the Hub with the exact pinned leaf supplied by the trusted offer;
//! the joining member receives the Fleet CA and CRL only after this TLS session is established.
#![allow(dead_code)] // transport primitives are wired to the Enroller in the next slice

use std::{
    collections::HashMap,
    future::Future,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rustls::{
    ClientConfig, DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::WebPkiSupportedAlgorithms,
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
    time::{Instant, sleep_until, timeout},
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

pub(crate) const FLEET_JOIN_PREFIX: &str = "BAFTFLEETJOIN1:";
pub(crate) const MAX_JOIN_FRAME: usize = 64 * 1024;
const MAX_JOIN_OFFER_BYTES: usize = 2048;
const JOIN_IO_TIMEOUT: Duration = Duration::from_secs(10);
const JOIN_MAX_CSR_BYTES: usize = 4096;
const JOIN_MAX_REASON_BYTES: usize = 256;
const JOIN_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const JOIN_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_JOIN_CONNECTIONS: usize = 8;
const MAX_JOIN_FAILURES_PER_IP: usize = 3;
const MAX_JOIN_FAILURES_TOTAL: usize = 32;

/// One-time, Fleet-specific offer delivered out-of-band by the operator.
/// Token and nonce are secrets: do not log this value or format it with Debug.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinOffer {
    pub v: u8,
    pub host: String,
    pub port: u16,
    pub hub_name: String,
    pub hub_id: String,
    pub hub_pin: String,
    pub offer_id: String,
    pub node_id: String,
    pub token: String,
    pub nonce: String,
    pub expires: u64,
}

impl JoinOffer {
    fn validate_fields(&self) -> Result<()> {
        ensure!(self.v == 1, "unsupported Fleet Join offer version");
        ensure!(self.port != 0, "Fleet Join offer port must be nonzero");
        ensure!(
            !self.host.is_empty() && self.host.len() <= 253 && self.host.trim() == self.host,
            "invalid Fleet Join dial host"
        );
        crate::tls::server_name(&self.host).context("invalid Fleet Join dial host")?;
        ensure!(
            !self.hub_name.is_empty()
                && self.hub_name.len() <= 253
                && self.hub_name.trim() == self.hub_name,
            "invalid Fleet Hub TLS name"
        );
        crate::tls::server_name(&self.hub_name).context("invalid Fleet Hub TLS name")?;
        ensure!(
            crate::fleet_ca::valid_node_id(&self.hub_id),
            "invalid Fleet Hub id"
        );
        ensure!(
            crate::fleet_ca::valid_node_id(&self.node_id),
            "invalid Fleet node id"
        );
        ensure!(
            self.hub_pin.len() == 64,
            "Fleet Hub pin must be a SHA-256 digest"
        );
        let pin = hex::decode(&self.hub_pin).context("Fleet Hub pin is not hexadecimal")?;
        ensure!(pin.len() == 32, "Fleet Hub pin must be a SHA-256 digest");
        ensure!(
            self.offer_id.len() == 16,
            "Fleet Join offer id must be 8 random bytes"
        );
        ensure!(
            hex::decode(&self.offer_id).is_ok(),
            "Fleet Join offer id is not hexadecimal"
        );
        for (name, value) in [("token", &self.token), ("nonce", &self.nonce)] {
            ensure!(value.len() == 32, "Fleet Join {name} must be 128 bits");
            ensure!(
                hex::decode(value).is_ok(),
                "Fleet Join {name} is not hexadecimal"
            );
        }
        ensure!(self.expires > 0, "Fleet Join offer expiry is invalid");
        Ok(())
    }

    pub(crate) fn validate(&self, now: u64) -> Result<()> {
        self.validate_fields()?;
        ensure!(self.expires >= now, "Fleet Join offer has expired");
        ensure!(
            self.expires <= now.saturating_add(crate::fleet_enroll::OFFER_TTL_SECS),
            "Fleet Join offer expiry exceeds the maximum TTL"
        );
        Ok(())
    }

    pub(crate) fn encode(&self) -> Result<String> {
        self.validate_fields()?;
        let bytes = serde_json::to_vec(self).context("encode Fleet Join offer")?;
        ensure!(
            bytes.len() <= MAX_JOIN_OFFER_BYTES,
            "Fleet Join offer is too large"
        );
        Ok(format!(
            "{FLEET_JOIN_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(bytes)
        ))
    }

    pub(crate) fn decode(code: &str, now: u64) -> Result<Self> {
        ensure!(
            code.starts_with(FLEET_JOIN_PREFIX),
            "not a Fleet Join offer"
        );
        ensure!(
            code.len() <= FLEET_JOIN_PREFIX.len() + (MAX_JOIN_OFFER_BYTES * 4).div_ceil(3),
            "Fleet Join offer is too large"
        );
        let encoded = &code[FLEET_JOIN_PREFIX.len()..];
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .context("invalid Fleet Join offer encoding")?;
        ensure!(
            bytes.len() <= MAX_JOIN_OFFER_BYTES,
            "Fleet Join offer is too large"
        );
        let offer: Self =
            serde_json::from_slice(&bytes).context("invalid Fleet Join offer JSON")?;
        offer.validate(now)?;
        ensure!(
            offer.encode()? == code,
            "Fleet Join offer encoding is not canonical"
        );
        Ok(offer)
    }
}

/// Member-to-Hub request. Private key material is never part of this message.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinRequest {
    pub offer_id: String,
    pub node_id: String,
    pub token: String,
    pub nonce: String,
    pub csr_pem: String,
}

impl JoinRequest {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.offer_id.len() == 16 && hex::decode(&self.offer_id).is_ok(),
            "invalid Fleet Join offer id"
        );
        ensure!(
            crate::fleet_ca::valid_node_id(&self.node_id),
            "invalid Fleet node id"
        );
        for (name, value) in [("token", &self.token), ("nonce", &self.nonce)] {
            ensure!(
                value.len() == 32 && hex::decode(value).is_ok(),
                "invalid Fleet Join {name}"
            );
        }
        ensure!(
            !self.csr_pem.is_empty() && self.csr_pem.len() <= JOIN_MAX_CSR_BYTES,
            "Fleet Join CSR is empty or too large"
        );
        Ok(())
    }
}

/// Hub response contains public certificates and trust material only.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinResponse {
    pub ok: bool,
    pub member_certificate_pem: Option<String>,
    pub ca_certificate_pem: Option<String>,
    pub crl_pem: Option<String>,
    pub hub_certificate_pem: Option<String>,
    pub reason: Option<String>,
}

impl JoinResponse {
    pub(crate) fn validate(&self) -> Result<()> {
        let has_all_public_material = self
            .member_certificate_pem
            .as_deref()
            .is_some_and(|value| !value.is_empty())
            && self
                .ca_certificate_pem
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            && self
                .crl_pem
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            && self
                .hub_certificate_pem
                .as_deref()
                .is_some_and(|value| !value.is_empty());
        if self.ok {
            ensure!(
                has_all_public_material,
                "successful Fleet Join response lacks public trust material"
            );
            ensure!(
                self.reason.is_none(),
                "successful Fleet Join response must not include a refusal reason"
            );
        } else {
            ensure!(
                self.member_certificate_pem.is_none()
                    && self.ca_certificate_pem.is_none()
                    && self.crl_pem.is_none()
                    && self.hub_certificate_pem.is_none(),
                "refused Fleet Join response must not carry certificates"
            );
            ensure!(
                self.reason
                    .as_ref()
                    .is_some_and(|v| !v.is_empty() && v.len() <= JOIN_MAX_REASON_BYTES),
                "Fleet Join refusal reason is missing or too long"
            );
        }
        Ok(())
    }
}

/// Final transport acknowledgement. This is not an Enrolled state transition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinAck {
    pub ok: bool,
}

pub(crate) async fn write_frame<W, T>(writer: &mut W, message: &T) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    write_frame_with_deadline(writer, message, JOIN_IO_TIMEOUT).await
}

async fn write_frame_with_deadline<W, T>(
    writer: &mut W,
    message: &T,
    deadline: Duration,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let bytes = serde_json::to_vec(message).context("serialize Fleet Join frame")?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_JOIN_FRAME,
        "Fleet Join frame is empty or too large"
    );
    let length = u32::try_from(bytes.len()).context("Fleet Join frame length overflow")?;
    timeout(deadline, async {
        writer.write_all(&length.to_be_bytes()).await?;
        writer.write_all(&bytes).await?;
        writer.flush().await
    })
    .await
    .map_err(|_| anyhow::anyhow!("Fleet Join frame write deadline exceeded"))?
    .context("write Fleet Join frame")?;
    Ok(())
}

pub(crate) async fn read_frame<R, T>(reader: &mut R) -> Result<T>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    read_frame_with_deadline(reader, JOIN_IO_TIMEOUT).await
}

async fn read_frame_with_deadline<R, T>(reader: &mut R, deadline: Duration) -> Result<T>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    timeout(deadline, async {
        let mut prefix = [0_u8; 4];
        reader
            .read_exact(&mut prefix)
            .await
            .context("read Fleet Join frame length")?;
        let length = u32::from_be_bytes(prefix) as usize;
        ensure!(length > 0, "Fleet Join frame length must be nonzero");
        ensure!(
            length <= MAX_JOIN_FRAME,
            "Fleet Join frame exceeds the maximum size"
        );
        let mut bytes = vec![0_u8; length];
        reader
            .read_exact(&mut bytes)
            .await
            .context("read Fleet Join frame body")?;
        serde_json::from_slice(&bytes).context("decode Fleet Join frame JSON")
    })
    .await
    .map_err(|_| anyhow::anyhow!("Fleet Join frame read deadline exceeded"))?
}

#[derive(Debug)]
struct HubPinVerifier {
    pin: [u8; 32],
    hub_id: String,
    hub_name: String,
    schemes: Vec<SignatureScheme>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for HubPinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let actual: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        if !intermediates.is_empty() {
            return Err(rustls::Error::General(
                "Fleet Join must present only the pinned Hub leaf certificate".to_owned(),
            ));
        }
        if actual != self.pin {
            return Err(rustls::Error::General(
                "Fleet Hub certificate pin mismatch".to_owned(),
            ));
        }
        if !server_name_matches(server_name, &self.hub_name) {
            return Err(rustls::Error::General(
                "Fleet Hub TLS server name mismatch".to_owned(),
            ));
        }
        verify_hub_tls_identity(
            end_entity.as_ref(),
            &self.hub_id,
            &self.hub_name,
            now.as_secs(),
        )
        .map_err(|error| {
            rustls::Error::General(format!(
                "Fleet Hub certificate identity is invalid: {error:#}"
            ))
        })?;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General(
            "TLS 1.2 is not allowed for Fleet Join".into(),
        ))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes.clone()
    }
}

fn server_name_matches(server_name: &ServerName<'_>, expected: &str) -> bool {
    if let Ok(expected_ip) = expected.parse::<IpAddr>() {
        matches!(server_name, ServerName::IpAddress(actual) if *actual == expected_ip.into())
    } else {
        matches!(server_name, ServerName::DnsName(actual) if actual.as_ref().eq_ignore_ascii_case(expected))
    }
}

/// Validate the pinned Fleet Hub leaf before the TLS handshake can carry application bytes.
/// The exact pin is the bootstrap trust anchor; CA/CRL are deliberately delivered later over this
/// authenticated session and are used for the persistent mandatory-mTLS Report connection.
fn verify_hub_tls_identity(leaf_der: &[u8], hub_id: &str, hub_name: &str, now: u64) -> Result<()> {
    use x509_parser::extensions::GeneralName;

    let (remaining, cert) = x509_parser::parse_x509_certificate(leaf_der)
        .map_err(|error| anyhow::anyhow!("parse pinned Fleet Hub certificate: {error}"))?;
    ensure!(
        remaining.is_empty(),
        "Fleet Hub certificate has trailing DER data"
    );
    let now = i64::try_from(now).context("TLS clock is outside the supported certificate range")?;
    ensure!(
        cert.validity().not_before.timestamp() <= now
            && cert.validity().not_after.timestamp() > now,
        "Fleet Hub certificate is not currently valid"
    );

    let common_names: Vec<_> = cert.subject().iter_common_name().collect();
    ensure!(
        common_names.len() == 1 && common_names[0].as_str().is_ok_and(|name| name == hub_id),
        "Fleet Hub certificate CN must exactly match hub_id"
    );
    let basic = cert
        .basic_constraints()?
        .context("Fleet Hub certificate lacks basicConstraints")?;
    ensure!(!basic.value.ca, "Fleet Hub certificate must not be a CA");
    let usage = cert
        .key_usage()?
        .context("Fleet Hub certificate lacks keyUsage")?;
    ensure!(
        usage.value.flags == 1,
        "Fleet Hub certificate keyUsage must be exactly digitalSignature"
    );
    let eku = cert
        .extended_key_usage()?
        .context("Fleet Hub certificate lacks extendedKeyUsage")?;
    ensure!(
        eku.value.server_auth
            && !eku.value.client_auth
            && !eku.value.any
            && !eku.value.code_signing
            && !eku.value.email_protection
            && !eku.value.time_stamping
            && !eku.value.ocsp_signing,
        "Fleet Hub certificate must be serverAuth-only outside its Fleet Hub EKU"
    );
    crate::fleet_report::verify_hub_identity(leaf_der, hub_id)?;

    let expected_uri = crate::fleet_ca::hub_uri(hub_id);
    let expected_ip = hub_name.parse::<IpAddr>().ok();
    let san = cert
        .subject_alternative_name()?
        .context("Fleet Hub certificate lacks subjectAltName")?;
    let mut uri_count = 0usize;
    let mut dial_name_count = 0usize;
    for name in &san.value.general_names {
        match name {
            GeneralName::URI(uri) if *uri == expected_uri => uri_count += 1,
            GeneralName::DNSName(dns)
                if expected_ip.is_none() && dns.eq_ignore_ascii_case(hub_name) =>
            {
                dial_name_count += 1;
            }
            GeneralName::IPAddress(bytes)
                if expected_ip.is_some_and(|ip| ip_san_matches(bytes, ip)) =>
            {
                dial_name_count += 1;
            }
            _ => bail!("Fleet Hub certificate contains an unexpected SAN entry"),
        }
    }
    ensure!(
        uri_count == 1,
        "Fleet Hub certificate must contain exactly one expected URI SAN"
    );
    ensure!(
        dial_name_count == 1,
        "Fleet Hub certificate must contain exactly one DNS/IP SAN matching hub_name"
    );
    Ok(())
}

fn ip_san_matches(bytes: &[u8], expected: IpAddr) -> bool {
    match expected {
        IpAddr::V4(ip) => bytes == ip.octets().as_slice(),
        IpAddr::V6(ip) => bytes == ip.octets().as_slice(),
    }
}

/// Build a TLS 1.3 client connector whose bootstrap trust anchor is the exact Hub leaf pin.
/// No CA/CRL is needed yet: those public trust materials arrive over the pinned Join session.
pub(crate) fn client_connector(
    pin_hex: &str,
    hub_id: &str,
    hub_name: &str,
) -> Result<TlsConnector> {
    ensure!(
        pin_hex.len() == 64,
        "Fleet Hub pin must be 64 hexadecimal characters"
    );
    let pin_bytes = hex::decode(pin_hex).context("Fleet Hub pin is not hexadecimal")?;
    let pin: [u8; 32] = pin_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("Fleet Hub pin must be a SHA-256 digest"))?;
    ensure!(
        crate::fleet_ca::valid_node_id(hub_id),
        "invalid Fleet Hub identity"
    );
    crate::tls::server_name(hub_name)?;

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let algorithms = provider.signature_verification_algorithms;
    let verifier = HubPinVerifier {
        pin,
        hub_id: hub_id.to_owned(),
        hub_name: hub_name.to_owned(),
        schemes: algorithms.supported_schemes(),
        algorithms,
    };
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.enable_early_data = false;
    Ok(TlsConnector::from(Arc::new(config)))
}

/// The admission controller owns the enrollment state machine. `Retry` is a bounded refusal
/// that permits another attempt; `Close` ends this one-shot offer; `Deliver` sends only public
/// certificates/trust material and waits for the member's transport acknowledgement.
pub(crate) enum Admission {
    Retry,
    Close,
    Deliver(JoinResponse),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionOutcome {
    Failed,
    Closed,
    Acknowledged,
}

/// A member-side authenticated Join session. The caller must persist the public certificate,
/// CA and CRL from `response()` before calling `ack(true)`.
pub(crate) struct JoinSession {
    stream: tokio_rustls::client::TlsStream<TcpStream>,
    response: JoinResponse,
}

impl JoinSession {
    pub(crate) fn response(&self) -> &JoinResponse {
        &self.response
    }

    /// Acknowledges successful local persistence (or explicitly refuses it with `false`).
    /// This is a transport acknowledgement only; it does not transition Enroller to Enrolled.
    pub(crate) async fn ack(mut self, ok: bool) -> Result<()> {
        write_frame(&mut self.stream, &JoinAck { ok }).await?;
        self.stream
            .shutdown()
            .await
            .context("close Fleet Join member session")?;
        Ok(())
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn refusal(reason: &str) -> JoinResponse {
    JoinResponse {
        ok: false,
        member_certificate_pem: None,
        ca_certificate_pem: None,
        crl_pem: None,
        hub_certificate_pem: None,
        reason: Some(reason.to_owned()),
    }
}

/// Connect as a joining member. TLS 1.3 and the exact Hub leaf pin/identity are verified before
/// the CSR is sent. The returned session keeps TLS open until the caller has persisted the public
/// trust bundle and sends its explicit acknowledgement.
pub(crate) async fn join(offer: &JoinOffer, csr_pem: &str) -> Result<JoinSession> {
    let now = unix_now();
    offer.validate(now)?;
    let remaining = Duration::from_secs(offer.expires.saturating_sub(now));
    ensure!(!remaining.is_zero(), "Fleet Join offer has expired");

    let request = JoinRequest {
        offer_id: offer.offer_id.clone(),
        node_id: offer.node_id.clone(),
        token: offer.token.clone(),
        nonce: offer.nonce.clone(),
        csr_pem: csr_pem.to_owned(),
    };
    request.validate()?;

    let tcp = timeout(
        remaining.min(JOIN_CONNECT_TIMEOUT),
        TcpStream::connect((offer.host.as_str(), offer.port)),
    )
    .await
    .context("Fleet Join connection deadline exceeded")?
    .context("connect to Fleet Hub for Join")?;

    let connector = client_connector(&offer.hub_pin, &offer.hub_id, &offer.hub_name)?;
    let remaining = Duration::from_secs(offer.expires.saturating_sub(unix_now()));
    ensure!(
        !remaining.is_zero(),
        "Fleet Join offer expired during connection setup"
    );
    let mut tls = timeout(
        remaining.min(JOIN_HANDSHAKE_TIMEOUT),
        connector.connect(crate::tls::server_name(&offer.hub_name)?, tcp),
    )
    .await
    .context("Fleet Join TLS handshake deadline exceeded")?
    .context("TLS handshake with Fleet Hub")?;

    let peer_leaf = tls
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .context("Fleet Hub presented no leaf certificate")?;
    ensure!(
        hex::encode(Sha256::digest(peer_leaf.as_ref())) == offer.hub_pin,
        "Fleet Hub TLS peer does not match the offered leaf pin"
    );
    write_frame(&mut tls, &request).await?;

    let remaining = Duration::from_secs(offer.expires.saturating_sub(unix_now()));
    ensure!(
        !remaining.is_zero(),
        "Fleet Join offer expired before trust delivery"
    );
    let response: JoinResponse = timeout(remaining, read_frame(&mut tls))
        .await
        .context("Fleet Join response deadline exceeded")??;
    response.validate()?;
    if !response.ok {
        bail!(
            "Fleet Hub refused Join: {}",
            response.reason.as_deref().unwrap_or("unspecified")
        );
    }

    let hub_pem = response
        .hub_certificate_pem
        .as_deref()
        .context("successful Fleet Join response has no Hub certificate")?;
    let (pem_tail, parsed_hub) = x509_parser::pem::parse_x509_pem(hub_pem.as_bytes())
        .map_err(|_| anyhow::anyhow!("delivered Fleet Hub certificate PEM is invalid"))?;
    ensure!(
        pem_tail.iter().all(|byte| byte.is_ascii_whitespace()),
        "delivered Hub certificate contains trailing or additional PEM data"
    );
    ensure!(
        parsed_hub.label == "CERTIFICATE",
        "delivered Hub PEM is not a certificate"
    );
    ensure!(
        hex::encode(Sha256::digest(&parsed_hub.contents)) == offer.hub_pin,
        "delivered Hub certificate does not match the authenticated TLS leaf"
    );
    verify_hub_tls_identity(
        &parsed_hub.contents,
        &offer.hub_id,
        &offer.hub_name,
        unix_now(),
    )?;

    // Recheck expiry after the response; the caller must not persist a bundle from an expired offer.
    ensure!(
        unix_now() <= offer.expires,
        "Fleet Join offer expired during the session"
    );
    Ok(JoinSession {
        stream: tls,
        response,
    })
}

async fn handle_join_connection<F, Fut>(
    tcp: TcpStream,
    peer: SocketAddr,
    acceptor: TlsAcceptor,
    offer_id: String,
    node_id: String,
    decide: Arc<F>,
) -> SessionOutcome
where
    F: Fn(JoinRequest, SocketAddr) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Admission> + Send + 'static,
{
    let Ok(Ok(mut tls)) = timeout(JOIN_HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await else {
        return SessionOutcome::Failed;
    };
    let Ok(request) = read_frame::<_, JoinRequest>(&mut tls).await else {
        return SessionOutcome::Failed;
    };
    if request.validate().is_err() || request.offer_id != offer_id || request.node_id != node_id {
        return SessionOutcome::Failed;
    }

    match decide(request, peer).await {
        Admission::Retry => {
            let _ = write_frame(&mut tls, &refusal("retry")).await;
            SessionOutcome::Failed
        }
        Admission::Close => {
            let _ = write_frame(&mut tls, &refusal("offer_closed")).await;
            SessionOutcome::Closed
        }
        Admission::Deliver(response) => {
            if !response.ok || response.validate().is_err() {
                return SessionOutcome::Failed;
            }
            if write_frame(&mut tls, &response).await.is_err() {
                return SessionOutcome::Failed;
            }
            match read_frame::<_, JoinAck>(&mut tls).await {
                Ok(JoinAck { ok: true }) => SessionOutcome::Acknowledged,
                _ => SessionOutcome::Failed,
            }
        }
    }
}

/// Serve one bounded Join offer. The listener exits successfully only after a member has received
/// the public trust bundle and sent `JoinAck { ok: true }`; no Enroller state is changed here.
pub(crate) async fn serve<F, Fut>(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    offer_id: String,
    node_id: String,
    expires: u64,
    decide: F,
) -> Result<()>
where
    F: Fn(JoinRequest, SocketAddr) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Admission> + Send + 'static,
{
    ensure!(
        offer_id.len() == 16 && hex::decode(&offer_id).is_ok(),
        "invalid Fleet Join offer id"
    );
    ensure!(
        crate::fleet_ca::valid_node_id(&node_id),
        "invalid Fleet node id"
    );
    let now = unix_now();
    ensure!(expires > now, "Fleet Join offer has expired");
    ensure!(
        expires <= now.saturating_add(crate::fleet_enroll::OFFER_TTL_SECS),
        "Fleet Join offer expiry exceeds the maximum TTL"
    );

    let deadline = Instant::now() + Duration::from_secs(expires - now);
    let acceptor = Arc::new(acceptor);
    let decide = Arc::new(decide);
    let slots = Arc::new(Semaphore::new(MAX_JOIN_CONNECTIONS));
    let mut sessions = JoinSet::new();
    let mut failures_by_ip: HashMap<IpAddr, usize> = HashMap::new();
    let mut total_failures = 0usize;

    loop {
        if total_failures >= MAX_JOIN_FAILURES_TOTAL {
            sessions.abort_all();
            bail!("Fleet Join offer exhausted its failure budget");
        }
        tokio::select! {
            _ = sleep_until(deadline) => {
                sessions.abort_all();
                bail!("Fleet Join offer expired before acknowledged delivery");
            }
            Some(joined) = sessions.join_next(), if !sessions.is_empty() => {
                match joined {
                    Ok((_ip, SessionOutcome::Acknowledged)) => {
                        sessions.abort_all();
                        return Ok(());
                    }
                    Ok((_, SessionOutcome::Closed)) => {
                        sessions.abort_all();
                        bail!("Fleet Join offer was closed by the admission controller");
                    }
                    Ok((ip, SessionOutcome::Failed)) => {
                        total_failures = total_failures.saturating_add(1);
                        *failures_by_ip.entry(ip).or_default() += 1;
                    }
                    Err(_) => total_failures = total_failures.saturating_add(1),
                }
            }
            accepted = listener.accept() => {
                let (tcp, peer) = accepted.context("accept Fleet Join connection")?;
                let ip = peer.ip();
                if failures_by_ip.get(&ip).copied().unwrap_or(0) >= MAX_JOIN_FAILURES_PER_IP {
                    drop(tcp);
                    continue;
                }
                let Ok(permit) = slots.clone().try_acquire_owned() else {
                    drop(tcp);
                    continue;
                };
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    drop(tcp);
                    continue;
                }
                let (acceptor, decide) = (acceptor.clone(), decide.clone());
                let (offer_id, node_id) = (offer_id.clone(), node_id.clone());
                sessions.spawn(async move {
                    let _permit = permit;
                    let outcome = match timeout(
                        remaining,
                        handle_join_connection(tcp, peer, (*acceptor).clone(), offer_id, node_id, decide),
                    ).await {
                        Ok(outcome) => outcome,
                        Err(_) => SessionOutcome::Failed,
                    };
                    (ip, outcome)
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    struct Material {
        root: PathBuf,
        cert: PathBuf,
        key: PathBuf,
        pin: String,
    }

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "baft-fleet-join-{tag}-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    fn hub_key_and_csr(dir: &Path, hub_id: &str) -> (PathBuf, String) {
        let key = crate::pair_ca::run(
            &[
                "genpkey",
                "-algorithm",
                "EC",
                "-pkeyopt",
                "ec_paramgen_curve:P-256",
            ],
            None,
        )
        .unwrap();
        let key_path = dir.join("hub.key");
        crate::pair_ca::write_private(&key_path, &key, 0o600).unwrap();
        let csr = crate::pair_ca::run(
            &[
                "req",
                "-new",
                "-key",
                &key_path.to_string_lossy(),
                "-subj",
                &format!("/CN={hub_id}"),
            ],
            None,
        )
        .unwrap();
        (key_path, String::from_utf8(csr).unwrap())
    }

    fn material(tag: &str, hub_id: &str, dial_san: &str) -> Material {
        let root = temp_dir(tag);
        std::fs::create_dir_all(&root).unwrap();
        let ca_dir = root.join("ca");
        crate::fleet_ca::create(&ca_dir, "Fleet Join Test CA").unwrap();
        let (key, csr) = hub_key_and_csr(&root, hub_id);
        let cert_pem = crate::fleet_ca::sign_hub(&ca_dir, hub_id, dial_san, &csr).unwrap();
        let cert = root.join("hub.crt");
        crate::pair_ca::write_private(&cert, cert_pem.as_bytes(), 0o644).unwrap();
        let der = crate::tls::certificates(&cert).unwrap()[0]
            .as_ref()
            .to_vec();
        Material {
            root,
            cert,
            key,
            pin: hex::encode(Sha256::digest(der)),
        }
    }

    fn custom_material(
        tag: &str,
        hub_id: &str,
        dial_san: &str,
        eku_oid: &str,
        validity: Option<(&str, &str)>,
    ) -> Material {
        custom_material_with_uri(tag, hub_id, hub_id, dial_san, eku_oid, validity)
    }

    fn custom_material_with_uri(
        tag: &str,
        cert_cn: &str,
        uri_hub_id: &str,
        dial_san: &str,
        eku_oid: &str,
        validity: Option<(&str, &str)>,
    ) -> Material {
        let root = temp_dir(tag);
        std::fs::create_dir_all(&root).unwrap();
        let ca_dir = root.join("ca");
        crate::fleet_ca::create(&ca_dir, "Fleet Join Test CA").unwrap();
        let (key, csr) = hub_key_and_csr(&root, cert_cn);
        let ext_path = root.join("hub.ext");
        let csr_path = root.join("hub.csr");
        let ext = format!(
            "[hub_ext]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,{eku_oid}\nsubjectAltName=URI:{},{}\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n",
            crate::fleet_ca::hub_uri(uri_hub_id),
            dial_san
        );
        crate::pair_ca::write_private(&ext_path, ext.as_bytes(), 0o600).unwrap();
        crate::pair_ca::write_private(&csr_path, csr.as_bytes(), 0o600).unwrap();
        let config = ca_dir.join("openssl.cnf");
        let mut args = vec![
            "ca".to_owned(),
            "-batch".to_owned(),
            "-notext".to_owned(),
            "-config".to_owned(),
            config.to_string_lossy().into_owned(),
            "-extfile".to_owned(),
            ext_path.to_string_lossy().into_owned(),
            "-extensions".to_owned(),
            "hub_ext".to_owned(),
        ];
        if let Some((start, end)) = validity {
            args.extend([
                "-startdate".to_owned(),
                start.to_owned(),
                "-enddate".to_owned(),
                end.to_owned(),
            ]);
        } else {
            args.extend(["-days".to_owned(), "30".to_owned()]);
        }
        args.extend([
            "-in".to_owned(),
            csr_path.to_string_lossy().into_owned(),
            "-out".to_owned(),
            "/dev/stdout".to_owned(),
        ]);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let cert_pem = crate::pair_ca::run(&refs, None).unwrap();
        let cert = root.join("hub-custom.crt");
        crate::pair_ca::write_private(&cert, &cert_pem, 0o644).unwrap();
        let der = crate::tls::certificates(&cert).unwrap()[0]
            .as_ref()
            .to_vec();
        Material {
            root,
            cert,
            key,
            pin: hex::encode(Sha256::digest(der)),
        }
    }

    async fn connect(
        material: &Material,
        pin: &str,
        hub_id: &str,
        hub_name: &str,
    ) -> anyhow::Result<()> {
        let acceptor = crate::tls::fleet_join_acceptor(&material.cert, &material.key)?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await?;
            let mut tls = acceptor.accept(tcp).await?;
            tls.write_all(b"ok").await?;
            anyhow::Ok(())
        });
        let connector = crate::fleet_join::client_connector(pin, hub_id, hub_name)?;
        let tcp = TcpStream::connect(address).await?;
        let name = crate::tls::server_name(hub_name)?;
        let mut tls = connector.connect(name, tcp).await?;
        anyhow::ensure!(
            tls.get_ref().1.protocol_version() == Some(rustls::ProtocolVersion::TLSv1_3),
            "Fleet Join negotiated a non-TLS-1.3 protocol"
        );
        let mut reply = [0u8; 2];
        tls.read_exact(&mut reply).await?;
        anyhow::ensure!(&reply == b"ok", "unexpected Join TLS test response");
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn fleet_join_tls13_accepts_exact_pinned_leaf_without_preloaded_ca_or_crl() {
        let m = material("server-only", "hub-1", "DNS:localhost");
        connect(&m, &m.pin, "hub-1", "localhost").await.unwrap();
        let _ = std::fs::remove_dir_all(m.root);
    }

    #[tokio::test]
    async fn fleet_join_rejects_wrong_leaf_pin() {
        let m = material("wrong-pin", "hub-1", "DNS:localhost");
        let wrong_pin = format!(
            "{}{}",
            if m.pin.starts_with('0') { "1" } else { "0" },
            &m.pin[1..]
        );
        let error = connect(&m, &wrong_pin, "hub-1", "localhost")
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Fleet Hub certificate pin mismatch"),
            "wrong-pin test failed for an unexpected reason: {error}"
        );
        let _ = std::fs::remove_dir_all(m.root);
    }

    #[tokio::test]
    async fn fleet_join_rejects_wrong_hub_id_uri_identity() {
        let m = material("wrong-hub-id", "hub-1", "DNS:localhost");
        assert!(connect(&m, &m.pin, "hub-2", "localhost").await.is_err());
        let _ = std::fs::remove_dir_all(m.root);
    }

    #[tokio::test]
    async fn fleet_join_rejects_wrong_dns_san() {
        let m = material("wrong-dns", "hub-1", "DNS:other.local");
        assert!(connect(&m, &m.pin, "hub-1", "localhost").await.is_err());
        let _ = std::fs::remove_dir_all(m.root);
    }

    #[tokio::test]
    async fn fleet_join_accepts_exact_ip_san_and_rejects_wrong_ip_san() {
        let valid = material("valid-ip", "hub-1", "IP:203.0.113.7");
        connect(&valid, &valid.pin, "hub-1", "203.0.113.7")
            .await
            .unwrap();
        let wrong = material("wrong-ip", "hub-1", "IP:203.0.113.7");
        assert!(
            connect(&wrong, &wrong.pin, "hub-1", "203.0.113.8")
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(valid.root);
        let _ = std::fs::remove_dir_all(wrong.root);
    }

    #[tokio::test]
    async fn fleet_join_rejects_expired_and_not_yet_valid_hub_certificates() {
        let expired = custom_material(
            "expired",
            "hub-1",
            "DNS:localhost",
            crate::fleet::EKU_FLEET_HUB,
            Some(("20000101000000Z", "20000102000000Z")),
        );
        assert!(
            connect(&expired, &expired.pin, "hub-1", "localhost")
                .await
                .is_err()
        );

        let future = custom_material(
            "not-yet-valid",
            "hub-1",
            "DNS:localhost",
            crate::fleet::EKU_FLEET_HUB,
            Some(("20270101000000Z", "20270102000000Z")),
        );
        assert!(
            connect(&future, &future.pin, "hub-1", "localhost")
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(expired.root);
        let _ = std::fs::remove_dir_all(future.root);
    }

    #[tokio::test]
    async fn fleet_join_rejects_wrong_uri_san_even_when_cn_matches() {
        let wrong_uri = custom_material_with_uri(
            "wrong-uri",
            "hub-2",
            "hub-1",
            "DNS:localhost",
            crate::fleet::EKU_FLEET_HUB,
            None,
        );
        assert!(
            connect(&wrong_uri, &wrong_uri.pin, "hub-2", "localhost")
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(wrong_uri.root);
    }

    #[tokio::test]
    async fn fleet_join_rejects_wrong_fleet_hub_eku() {
        let wrong_eku = custom_material(
            "wrong-eku",
            "hub-1",
            "DNS:localhost",
            crate::fleet::EKU_FLEET_NODE,
            None,
        );
        assert!(
            connect(&wrong_eku, &wrong_eku.pin, "hub-1", "localhost")
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(wrong_eku.root);
    }

    fn unix_now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn join_offer(material: &Material, address: std::net::SocketAddr, expires: u64) -> JoinOffer {
        JoinOffer {
            v: 1,
            host: address.ip().to_string(),
            port: address.port(),
            hub_name: "localhost".into(),
            hub_id: "hub-1".into(),
            hub_pin: material.pin.clone(),
            offer_id: "b".repeat(16),
            node_id: "node-1".into(),
            token: "c".repeat(32),
            nonce: "d".repeat(32),
            expires,
        }
    }

    fn join_response(material: &Material, node_id: &str, csr: &str) -> JoinResponse {
        let ca_dir = material.root.join("ca");
        JoinResponse {
            ok: true,
            member_certificate_pem: Some(crate::fleet_ca::sign(&ca_dir, node_id, csr).unwrap()),
            ca_certificate_pem: Some(std::fs::read_to_string(ca_dir.join("ca.crt")).unwrap()),
            crl_pem: Some(std::fs::read_to_string(ca_dir.join("ca.crl")).unwrap()),
            hub_certificate_pem: Some(std::fs::read_to_string(&material.cert).unwrap()),
            reason: None,
        }
    }

    fn member_csr(material: &Material) -> String {
        let member_dir = material.root.join("member");
        std::fs::create_dir_all(&member_dir).unwrap();
        let (_private_key_path, csr) = hub_key_and_csr(&member_dir, "node-1");
        csr
    }

    #[tokio::test]
    async fn fleet_join_loopback_delivers_public_trust_and_waits_for_member_ack() {
        let material = material("loopback-join", "hub-1", "DNS:localhost");
        let csr = member_csr(&material);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expires = unix_now() + 30;
        let offer = join_offer(&material, address, expires);
        let response = std::sync::Arc::new(join_response(&material, "node-1", &csr));
        let acceptor = crate::tls::fleet_join_acceptor(&material.cert, &material.key).unwrap();
        let offer_id = offer.offer_id.clone();
        let node_id = offer.node_id.clone();
        let expected_token = offer.token.clone();
        let expected_nonce = offer.nonce.clone();
        let response_for_server = response.clone();
        let server = tokio::spawn(serve(
            listener,
            acceptor,
            offer_id,
            node_id,
            expires,
            move |request, _peer| {
                let response = response_for_server.clone();
                let token = expected_token.clone();
                let nonce = expected_nonce.clone();
                async move {
                    assert_eq!(request.node_id, "node-1");
                    assert_eq!(request.token, token);
                    assert_eq!(request.nonce, nonce);
                    Admission::Deliver((*response).clone())
                }
            },
        ));

        let session = join(&offer, &csr).await.unwrap();
        assert_eq!(
            session.response().member_certificate_pem.as_deref(),
            response.member_certificate_pem.as_deref()
        );
        assert_eq!(
            session.response().ca_certificate_pem.as_deref(),
            response.ca_certificate_pem.as_deref()
        );
        assert_eq!(
            session.response().crl_pem.as_deref(),
            response.crl_pem.as_deref()
        );
        session.ack(true).await.unwrap();
        server.await.unwrap().unwrap();
        let _ = std::fs::remove_dir_all(material.root);
    }

    #[tokio::test]
    async fn fleet_join_client_rejects_wrong_pin_and_wrong_hub_identity_before_request() {
        for (tag, wrong_pin, wrong_hub) in [
            ("wrong-pin-network", true, false),
            ("wrong-hub-network", false, true),
        ] {
            let material = material(tag, "hub-1", "DNS:localhost");
            let csr = member_csr(&material);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let expires = unix_now() + 3;
            let mut offer = join_offer(&material, address, expires);
            if wrong_pin {
                offer.hub_pin = format!(
                    "{}{}",
                    if material.pin.starts_with('0') {
                        "1"
                    } else {
                        "0"
                    },
                    &material.pin[1..]
                );
            }
            if wrong_hub {
                offer.hub_id = "hub-2".into();
            }
            let acceptor = crate::tls::fleet_join_acceptor(&material.cert, &material.key).unwrap();
            let server = tokio::spawn(serve(
                listener,
                acceptor,
                offer.offer_id.clone(),
                offer.node_id.clone(),
                expires,
                |_request, _peer| async {
                    panic!("invalid Hub identity must fail before CSR submission")
                },
            ));
            assert!(join(&offer, &csr).await.is_err());
            server.abort();
            let _ = std::fs::remove_dir_all(material.root);
        }
    }

    #[tokio::test]
    async fn fleet_join_rejects_expired_offer_before_opening_a_connection() {
        let material = material("expired-offer-network", "hub-1", "DNS:localhost");
        let csr = member_csr(&material);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let offer = join_offer(
            &material,
            listener.local_addr().unwrap(),
            unix_now().saturating_sub(1),
        );
        assert!(join(&offer, &csr).await.is_err());
        drop(listener);
        let _ = std::fs::remove_dir_all(material.root);
    }

    #[tokio::test]
    async fn fleet_join_server_refuses_wrong_offer_id() {
        let material = material("wrong-offer-network", "hub-1", "DNS:localhost");
        let csr = member_csr(&material);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expires = unix_now() + 30;
        let offer = join_offer(&material, address, expires);
        let acceptor = crate::tls::fleet_join_acceptor(&material.cert, &material.key).unwrap();
        let server = tokio::spawn(serve(
            listener,
            acceptor,
            "e".repeat(16),
            "node-1".into(),
            expires,
            |_request, _peer| async { Admission::Close },
        ));
        assert!(join(&offer, &csr).await.is_err());
        server.abort();
        let _ = std::fs::remove_dir_all(material.root);
    }

    async fn raw_tls_client(offer: &JoinOffer) -> tokio_rustls::client::TlsStream<TcpStream> {
        let connector = client_connector(&offer.hub_pin, &offer.hub_id, &offer.hub_name).unwrap();
        let tcp = TcpStream::connect((offer.host.as_str(), offer.port))
            .await
            .unwrap();
        connector
            .connect(crate::tls::server_name(&offer.hub_name).unwrap(), tcp)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn fleet_join_server_retries_bad_token_and_nonce_without_delivering_certificates() {
        for (tag, bad_token, bad_nonce) in [
            ("bad-token-network", true, false),
            ("bad-nonce-network", false, true),
        ] {
            let material = material(tag, "hub-1", "DNS:localhost");
            let csr = member_csr(&material);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let expires = unix_now() + 30;
            let mut offer = join_offer(&material, address, expires);
            if bad_token {
                offer.token = "e".repeat(32);
            }
            if bad_nonce {
                offer.nonce = "f".repeat(32);
            }
            let acceptor = crate::tls::fleet_join_acceptor(&material.cert, &material.key).unwrap();
            let server = tokio::spawn(serve(
                listener,
                acceptor,
                offer.offer_id.clone(),
                offer.node_id.clone(),
                expires,
                |_request, _peer| async { Admission::Retry },
            ));
            assert!(join(&offer, &csr).await.is_err());
            server.abort();
            let _ = std::fs::remove_dir_all(material.root);
        }
    }

    #[tokio::test]
    async fn fleet_join_server_rejects_wrong_node_identity() {
        let material = material("wrong-node-network", "hub-1", "DNS:localhost");
        let csr = member_csr(&material);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expires = unix_now() + 30;
        let mut offer = join_offer(&material, address, expires);
        offer.node_id = "node-2".into();
        let acceptor = crate::tls::fleet_join_acceptor(&material.cert, &material.key).unwrap();
        let server = tokio::spawn(serve(
            listener,
            acceptor,
            offer.offer_id.clone(),
            "node-1".into(),
            expires,
            |_request, _peer| async {
                panic!("wrong node identity must be rejected before admission")
            },
        ));
        assert!(join(&offer, &csr).await.is_err());
        server.abort();
        let _ = std::fs::remove_dir_all(material.root);
    }

    #[tokio::test]
    async fn fleet_join_replay_and_concurrent_submission_deliver_only_once() {
        let material = material("replay-network", "hub-1", "DNS:localhost");
        let csr = member_csr(&material);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expires = unix_now() + 30;
        let offer = join_offer(&material, address, expires);
        let response = std::sync::Arc::new(join_response(&material, "node-1", &csr));
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let acceptor = crate::tls::fleet_join_acceptor(&material.cert, &material.key).unwrap();
        let response_for_server = response.clone();
        let attempts_for_server = attempts.clone();
        let server = tokio::spawn(serve(
            listener,
            acceptor,
            offer.offer_id.clone(),
            offer.node_id.clone(),
            expires,
            move |_request, _peer| {
                let response = response_for_server.clone();
                let attempt = attempts_for_server.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    if attempt == 0 {
                        Admission::Deliver((*response).clone())
                    } else {
                        Admission::Retry
                    }
                }
            },
        ));
        let (first, second) = tokio::join!(join(&offer, &csr), join(&offer, &csr));
        assert_ne!(first.is_ok(), second.is_ok());
        let session = first.or(second).unwrap();
        session.ack(true).await.unwrap();
        server.await.unwrap().unwrap();
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(material.root);
    }

    #[tokio::test]
    async fn fleet_join_rejects_oversized_and_stalled_frames_without_success() {
        for (tag, oversized) in [("oversized-frame", true), ("stalled-frame", false)] {
            let material = material(tag, "hub-1", "DNS:localhost");
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let expires = unix_now() + 2;
            let offer = join_offer(&material, address, expires);
            let acceptor = crate::tls::fleet_join_acceptor(&material.cert, &material.key).unwrap();
            let server = tokio::spawn(serve(
                listener,
                acceptor,
                offer.offer_id.clone(),
                offer.node_id.clone(),
                expires,
                |_request, _peer| async { panic!("invalid frame must not reach admission") },
            ));
            let mut tls = raw_tls_client(&offer).await;
            if oversized {
                tls.write_all(&((MAX_JOIN_FRAME as u32) + 1).to_be_bytes())
                    .await
                    .unwrap();
            }
            // For the slow case, keep the authenticated socket open without sending a frame.
            assert!(
                timeout(std::time::Duration::from_secs(4), server)
                    .await
                    .unwrap()
                    .unwrap()
                    .is_err()
            );
            drop(tls);
            let _ = std::fs::remove_dir_all(material.root);
        }
    }

    #[tokio::test]
    async fn fleet_join_disconnect_before_response_never_reports_delivery_success() {
        let material = material("disconnect-before-response", "hub-1", "DNS:localhost");
        let csr = member_csr(&material);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expires = unix_now() + 2;
        let offer = join_offer(&material, address, expires);
        let response = std::sync::Arc::new(join_response(&material, "node-1", &csr));
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let receiver = std::sync::Arc::new(std::sync::Mutex::new(Some(release_rx)));
        let acceptor = crate::tls::fleet_join_acceptor(&material.cert, &material.key).unwrap();
        let response_for_server = response.clone();
        let receiver_for_server = receiver.clone();
        let server = tokio::spawn(serve(
            listener,
            acceptor,
            offer.offer_id.clone(),
            offer.node_id.clone(),
            expires,
            move |_request, _peer| {
                let response = response_for_server.clone();
                let receiver = receiver_for_server.lock().unwrap().take().unwrap();
                async move {
                    let _ = receiver.await;
                    Admission::Deliver((*response).clone())
                }
            },
        ));
        let mut tls = raw_tls_client(&offer).await;
        let request = JoinRequest {
            offer_id: offer.offer_id.clone(),
            node_id: offer.node_id.clone(),
            token: offer.token.clone(),
            nonce: offer.nonce.clone(),
            csr_pem: csr,
        };
        write_frame(&mut tls, &request).await.unwrap();
        drop(tls);
        let _ = release_tx.send(());
        assert!(
            timeout(std::time::Duration::from_secs(4), server)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        let _ = std::fs::remove_dir_all(material.root);
    }

    #[tokio::test]
    async fn fleet_join_member_rejects_hub_certificate_bundle_not_matching_tls_pin() {
        let material = material("wrong-bundle-hub-cert", "hub-1", "DNS:localhost");
        let csr = member_csr(&material);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expires = unix_now() + 30;
        let offer = join_offer(&material, address, expires);
        let mut response = join_response(&material, "node-1", &csr);
        response.hub_certificate_pem =
            Some("-----BEGIN CERTIFICATE-----\nZmFrZQ==\n-----END CERTIFICATE-----\n".into());
        let response = std::sync::Arc::new(response);
        let acceptor = crate::tls::fleet_join_acceptor(&material.cert, &material.key).unwrap();
        let response_for_server = response.clone();
        let server = tokio::spawn(serve(
            listener,
            acceptor,
            offer.offer_id.clone(),
            offer.node_id.clone(),
            expires,
            move |_request, _peer| {
                let response = response_for_server.clone();
                async move { Admission::Deliver((*response).clone()) }
            },
        ));
        assert!(join(&offer, &csr).await.is_err());
        server.abort();
        let _ = std::fs::remove_dir_all(material.root);
    }

    #[tokio::test]
    async fn fleet_join_server_closes_offer_when_admission_controller_closes_it() {
        let material = material("closed-offer-network", "hub-1", "DNS:localhost");
        let csr = member_csr(&material);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expires = unix_now() + 30;
        let offer = join_offer(&material, address, expires);
        let acceptor = crate::tls::fleet_join_acceptor(&material.cert, &material.key).unwrap();
        let server = tokio::spawn(serve(
            listener,
            acceptor,
            offer.offer_id.clone(),
            offer.node_id.clone(),
            expires,
            |_request, _peer| async { Admission::Close },
        ));
        assert!(join(&offer, &csr).await.is_err());
        assert!(server.await.unwrap().is_err());
        let _ = std::fs::remove_dir_all(material.root);
    }

    #[tokio::test]
    async fn fleet_join_server_does_not_mark_delivery_success_without_member_ack() {
        let material = material("missing-ack-network", "hub-1", "DNS:localhost");
        let csr = member_csr(&material);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expires = unix_now() + 2;
        let offer = join_offer(&material, address, expires);
        let response = std::sync::Arc::new(join_response(&material, "node-1", &csr));
        let acceptor = crate::tls::fleet_join_acceptor(&material.cert, &material.key).unwrap();
        let response_for_server = response.clone();
        let server = tokio::spawn(serve(
            listener,
            acceptor,
            offer.offer_id.clone(),
            offer.node_id.clone(),
            expires,
            move |_request, _peer| {
                let response = response_for_server.clone();
                async move { Admission::Deliver((*response).clone()) }
            },
        ));
        let session = join(&offer, &csr).await.unwrap();
        drop(session);
        assert!(
            timeout(std::time::Duration::from_secs(4), server)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        let _ = std::fs::remove_dir_all(material.root);
    }
}

#[cfg(test)]
mod framing_tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, duplex};
    use tokio::time::Duration;

    fn offer() -> JoinOffer {
        JoinOffer {
            v: 1,
            host: "198.51.100.10".into(),
            port: 7443,
            hub_name: "hub-1.fleet.baft.invalid".into(),
            hub_id: "hub-1".into(),
            hub_pin: "a".repeat(64),
            offer_id: "b".repeat(16),
            node_id: "node-1".into(),
            token: "c".repeat(32),
            nonce: "d".repeat(32),
            expires: 1_700_000_900,
        }
    }

    fn request() -> JoinRequest {
        JoinRequest {
            offer_id: "b".repeat(16),
            node_id: "node-1".into(),
            token: "c".repeat(32),
            nonce: "d".repeat(32),
            csr_pem: "-----BEGIN CERTIFICATE REQUEST-----\nTEST\n-----END CERTIFICATE REQUEST-----"
                .into(),
        }
    }

    #[test]
    fn offer_round_trips_with_fleet_only_prefix_and_expiry_validation() {
        let offer = offer();
        offer.validate(1_700_000_000).unwrap();
        let encoded = offer.encode().unwrap();
        assert!(encoded.starts_with(FLEET_JOIN_PREFIX));
        assert!(!encoded.contains("BAFTPAIR"));
        let decoded = JoinOffer::decode(&encoded, 1_700_000_000).unwrap();
        assert!(decoded == offer);
        assert!(JoinOffer::decode(&encoded, 1_700_001_000).is_err());
    }

    #[test]
    fn offer_rejects_invalid_identity_pin_token_nonce_host_port_and_ttl() {
        let now = 1_700_000_000;
        let mut bad = offer();
        bad.hub_pin = "z".repeat(64);
        assert!(bad.validate(now).is_err());
        let mut bad = offer();
        bad.token = "c".repeat(30);
        assert!(bad.validate(now).is_err());
        let mut bad = offer();
        bad.nonce = "d".repeat(30);
        assert!(bad.validate(now).is_err());
        let mut bad = offer();
        bad.hub_id = "not valid".into();
        assert!(bad.validate(now).is_err());
        let mut bad = offer();
        bad.node_id = "not valid".into();
        assert!(bad.validate(now).is_err());
        let mut bad = offer();
        bad.host = "not a host".into();
        assert!(bad.validate(now).is_err());
        let mut bad = offer();
        bad.port = 0;
        assert!(bad.validate(now).is_err());
        let mut bad = offer();
        bad.expires = now + 901;
        assert!(bad.validate(now).is_err());
    }

    #[test]
    fn fleet_join_offer_rejects_unknown_and_preloaded_trust_bundle_fields() {
        let mut value = serde_json::to_value(offer()).unwrap();
        value["ca_certificate_pem"] = serde_json::Value::String("not allowed before Join".into());
        let bytes = serde_json::to_vec(&value).unwrap();
        let code = format!("{FLEET_JOIN_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
        assert!(JoinOffer::decode(&code, 1_700_000_000).is_err());
    }

    #[test]
    fn join_request_and_response_validate_bounds_and_public_material_only() {
        request().validate().unwrap();
        let mut oversized = request();
        oversized.csr_pem = "x".repeat(JOIN_MAX_CSR_BYTES + 1);
        assert!(oversized.validate().is_err());

        let success = JoinResponse {
            ok: true,
            member_certificate_pem: Some("member public cert".into()),
            ca_certificate_pem: Some("public CA cert".into()),
            crl_pem: Some("public CRL".into()),
            hub_certificate_pem: Some("public Hub cert".into()),
            reason: None,
        };
        success.validate().unwrap();

        let refused = JoinResponse {
            ok: false,
            member_certificate_pem: None,
            ca_certificate_pem: None,
            crl_pem: None,
            hub_certificate_pem: None,
            reason: Some("operator denied".into()),
        };
        refused.validate().unwrap();

        let mut unsafe_refusal = refused;
        unsafe_refusal.ca_certificate_pem = Some("unexpected cert".into());
        assert!(unsafe_refusal.validate().is_err());
    }

    #[test]
    fn fleet_join_messages_reject_unknown_json_fields() {
        assert!(serde_json::from_str::<JoinRequest>(
            r#"{"offer_id":"bbbbbbbbbbbbbbbb","node_id":"node-1","token":"cccccccccccccccccccccccccccccccc","nonce":"dddddddddddddddddddddddddddddddd","csr_pem":"x","extra":true}"#
        ).is_err());
        assert!(serde_json::from_str::<JoinResponse>(
            r#"{"ok":false,"member_certificate_pem":null,"ca_certificate_pem":null,"crl_pem":null,"hub_certificate_pem":null,"reason":"denied","extra":true}"#
        ).is_err());
        assert!(serde_json::from_str::<JoinAck>(r#"{"ok":true,"extra":true}"#).is_err());
    }

    #[tokio::test]
    async fn bounded_frame_round_trip_preserves_strict_join_request() {
        let (mut writer, mut reader) = duplex(4096);
        let expected = request();
        write_frame(&mut writer, &expected).await.unwrap();
        let actual: JoinRequest = read_frame(&mut reader).await.unwrap();
        assert!(actual == expected);
    }

    #[tokio::test]
    async fn frame_reader_rejects_zero_and_oversized_lengths_before_allocation() {
        for length in [0_u32, MAX_JOIN_FRAME as u32 + 1] {
            let (mut writer, mut reader) = duplex(16);
            writer.write_all(&length.to_be_bytes()).await.unwrap();
            assert!(read_frame::<_, JoinRequest>(&mut reader).await.is_err());
        }
    }

    #[tokio::test]
    async fn frame_reader_rejects_truncated_payload() {
        let (mut writer, mut reader) = duplex(16);
        writer.write_all(&5_u32.to_be_bytes()).await.unwrap();
        writer.write_all(b"{} ").await.unwrap();
        drop(writer);
        assert!(read_frame::<_, JoinRequest>(&mut reader).await.is_err());
    }

    #[tokio::test]
    async fn frame_reader_has_a_bounded_deadline_when_peer_stalls() {
        let (_writer, mut reader) = duplex(16);
        assert!(
            read_frame_with_deadline::<_, JoinRequest>(&mut reader, Duration::from_millis(25))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn frame_writer_rejects_oversized_serialized_payload_before_writing() {
        let (mut writer, _reader) = duplex(16);
        let oversized = "x".repeat(MAX_JOIN_FRAME + 1);
        assert!(write_frame(&mut writer, &oversized).await.is_err());
    }

    #[tokio::test]
    async fn frame_writer_has_a_bounded_deadline_when_peer_does_not_read() {
        let (mut writer, _reader) = duplex(1);
        assert!(
            write_frame_with_deadline(&mut writer, &request(), Duration::from_millis(25))
                .await
                .is_err()
        );
    }
}
