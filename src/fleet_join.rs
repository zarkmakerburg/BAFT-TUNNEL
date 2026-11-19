//! Fleet Join TLS transport primitives (ADR-0051).
//!
//! This module is intentionally separate from tunnel pairing and the persistent Fleet Report
//! listener. Join authenticates the Hub with the exact pinned leaf supplied by the trusted offer;
//! the joining member receives the Fleet CA and CRL only after this TLS session is established.
#![allow(dead_code)] // transport primitives are wired to the Enroller in the next slice

use std::{net::IpAddr, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use rustls::{
    ClientConfig, DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::WebPkiSupportedAlgorithms,
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use sha2::{Digest, Sha256};
use tokio_rustls::TlsConnector;

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
