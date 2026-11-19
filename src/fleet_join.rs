//! Fleet Join TLS transport primitives (ADR-0051).
//!
//! This module is intentionally separate from tunnel pairing and the persistent Fleet Report
//! listener. Join authenticates the Hub with the exact pinned leaf supplied by the trusted offer;
//! the joining member receives the Fleet CA and CRL only after this TLS session is established.
#![allow(dead_code)] // transport primitives are wired to the Enroller in the next slice

use std::{net::IpAddr, sync::Arc, time::Duration};

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
    time::timeout,
};
use tokio_rustls::TlsConnector;

pub(crate) const FLEET_JOIN_PREFIX: &str = "BAFTFLEETJOIN1:";
pub(crate) const MAX_JOIN_FRAME: usize = 64 * 1024;
const MAX_JOIN_OFFER_BYTES: usize = 2048;
const JOIN_IO_TIMEOUT: Duration = Duration::from_secs(2);
const JOIN_MAX_CSR_BYTES: usize = 4096;
const JOIN_MAX_REASON_BYTES: usize = 256;

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
        let public_material = [
            &self.member_certificate_pem,
            &self.ca_certificate_pem,
            &self.crl_pem,
            &self.hub_certificate_pem,
        ];
        if self.ok {
            ensure!(
                public_material
                    .iter()
                    .all(|item| item.as_ref().is_some_and(|v| !v.is_empty())),
                "successful Fleet Join response lacks public trust material"
            );
            ensure!(
                self.reason.is_none(),
                "successful Fleet Join response must not include a refusal reason"
            );
        } else {
            ensure!(
                public_material.iter().all(|item| item.is_none()),
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
    let bytes = serde_json::to_vec(message).context("serialize Fleet Join frame")?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_JOIN_FRAME,
        "Fleet Join frame is empty or too large"
    );
    let length = u32::try_from(bytes.len()).context("Fleet Join frame length overflow")?;
    timeout(JOIN_IO_TIMEOUT, async {
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
    timeout(JOIN_IO_TIMEOUT, async {
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

#[cfg(test)]
mod tests {
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
}

#[cfg(test)]
mod framing_tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, duplex};
    use tokio::time::{Duration, timeout};

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
            timeout(
                Duration::from_secs(3),
                read_frame::<_, JoinRequest>(&mut reader)
            )
            .await
            .unwrap()
            .is_err()
        );
    }
}
