//! Fleet Join TLS transport primitives (ADR-0051).
//!
//! This module is intentionally separate from tunnel pairing and the persistent Fleet Report
//! listener. Join uses TLS 1.3 server authentication only; the member authenticates the Hub CA
//! chain, CRL, dial target and exact leaf pin before sending its CSR.

#![allow(dead_code)] // transport primitives are wired to the Enroller in the next slice

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, ensure};
use rustls::{
    ClientConfig, DigitallySignedStruct, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use sha2::{Digest, Sha256};
use tokio_rustls::TlsConnector;

#[derive(Debug)]
struct HubPinVerifier {
    webpki: Arc<WebPkiServerVerifier>,
    pin: [u8; 32],
}

impl ServerCertVerifier for HubPinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let verified = self.webpki.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        )?;
        let actual: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        if actual != self.pin {
            return Err(rustls::Error::General(
                "Fleet Hub certificate pin mismatch".to_owned(),
            ));
        }
        Ok(verified)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.webpki.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.webpki.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.webpki.supported_verify_schemes()
    }
}

/// Build a TLS 1.3 client connector with CA/CRL verification and an exact Hub leaf pin.
/// The caller supplies the dial target as `ServerName` during `connect`, then verifies the
/// Fleet Hub EKU and URI SAN before sending any application bytes.
pub(crate) fn client_connector(ca: &Path, crl: &Path, pin_hex: &str) -> Result<TlsConnector> {
    ensure!(
        pin_hex.len() == 64,
        "Fleet Hub pin must be 64 hexadecimal characters"
    );
    let pin_bytes = hex::decode(pin_hex).context("Fleet Hub pin is not hexadecimal")?;
    let pin: [u8; 32] = pin_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("Fleet Hub pin must be a SHA-256 digest"))?;
    let webpki = crate::tls::fleet_join_server_verifier(ca, crl)?;
    let mut config = ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(HubPinVerifier { webpki, pin }))
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
        ca_dir: PathBuf,
        ca: PathBuf,
        crl: PathBuf,
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
        let pin = hex::encode(Sha256::digest(der));
        Material {
            ca: ca_dir.join("ca.crt"),
            crl: ca_dir.join("ca.crl"),
            ca_dir,
            cert,
            key,
            pin,
            root,
        }
    }

    async fn connect(
        material: &Material,
        ca: &Path,
        crl: &Path,
        pin: &str,
        server_name: &str,
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
        let connector = crate::fleet_join::client_connector(ca, crl, pin)?;
        let tcp = TcpStream::connect(address).await?;
        let name = crate::tls::server_name(server_name)?;
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

    fn expired_material(tag: &str, hub_id: &str, dial_san: &str) -> Material {
        let root = temp_dir(tag);
        std::fs::create_dir_all(&root).unwrap();
        let ca_dir = root.join("ca");
        crate::fleet_ca::create(&ca_dir, "Fleet Join Test CA").unwrap();
        let (key, csr) = hub_key_and_csr(&root, hub_id);
        let ext_path = root.join("hub.ext");
        let csr_path = root.join("hub.csr");
        let ext = format!(
            "[hub_ext]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,{}\nsubjectAltName=URI:{},{}\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n",
            crate::fleet::EKU_FLEET_HUB,
            crate::fleet_ca::hub_uri(hub_id),
            dial_san
        );
        crate::pair_ca::write_private(&ext_path, ext.as_bytes(), 0o600).unwrap();
        crate::pair_ca::write_private(&csr_path, csr.as_bytes(), 0o600).unwrap();
        let config = ca_dir.join("openssl.cnf");
        let cert_pem = crate::pair_ca::run(
            &[
                "ca",
                "-batch",
                "-notext",
                "-config",
                &config.to_string_lossy(),
                "-extfile",
                &ext_path.to_string_lossy(),
                "-extensions",
                "hub_ext",
                "-startdate",
                "20000101000000Z",
                "-enddate",
                "20000102000000Z",
                "-in",
                &csr_path.to_string_lossy(),
                "-out",
                "/dev/stdout",
            ],
            None,
        )
        .unwrap();
        let cert = root.join("hub-expired.crt");
        crate::pair_ca::write_private(&cert, &cert_pem, 0o644).unwrap();
        let der = crate::tls::certificates(&cert).unwrap()[0]
            .as_ref()
            .to_vec();
        Material {
            ca: ca_dir.join("ca.crt"),
            crl: ca_dir.join("ca.crl"),
            ca_dir,
            cert,
            key,
            pin: hex::encode(Sha256::digest(der)),
            root,
        }
    }

    #[tokio::test]
    async fn fleet_join_tls13_accepts_server_auth_only_client_with_exact_pin() {
        let m = material("server-only", "hub-1", "DNS:localhost");
        connect(&m, &m.ca, &m.crl, &m.pin, "localhost")
            .await
            .unwrap();
        let _ = std::fs::remove_dir_all(m.root);
    }

    #[tokio::test]
    async fn fleet_join_rejects_wrong_leaf_pin_and_wrong_ca() {
        let m = material("wrong-pin", "hub-1", "DNS:localhost");
        let wrong_pin = format!(
            "{}{}",
            if m.pin.starts_with('0') { "1" } else { "0" },
            &m.pin[1..]
        );
        assert!(
            connect(&m, &m.ca, &m.crl, &wrong_pin, "localhost")
                .await
                .is_err()
        );

        let other = material("wrong-ca", "hub-2", "DNS:localhost");
        assert!(
            connect(&m, &other.ca, &other.crl, &m.pin, "localhost")
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(m.root);
        let _ = std::fs::remove_dir_all(other.root);
    }

    #[tokio::test]
    async fn fleet_join_rejects_wrong_dial_target_san() {
        let m = material("wrong-san", "hub-1", "DNS:other.local");
        assert!(
            connect(&m, &m.ca, &m.crl, &m.pin, "localhost")
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(m.root);
    }

    #[tokio::test]
    async fn fleet_join_rejects_expired_hub_certificate() {
        let m = expired_material("expired", "hub-1", "DNS:localhost");
        assert!(
            connect(&m, &m.ca, &m.crl, &m.pin, "localhost")
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(m.root);
    }

    #[test]
    fn fleet_hub_identity_requires_the_exact_hub_eku_and_uri_san() {
        let hub = material("hub-identity", "hub-1", "DNS:localhost");
        let leaf = crate::tls::certificates(&hub.cert).unwrap()[0]
            .as_ref()
            .to_vec();
        crate::fleet_report::verify_hub_identity(&leaf, "hub-1").unwrap();
        assert!(crate::fleet_report::verify_hub_identity(&leaf, "hub-2").is_err());

        let node_key = crate::pair_ca::run(
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
        let node_key_path = hub.root.join("node.key");
        crate::pair_ca::write_private(&node_key_path, &node_key, 0o600).unwrap();
        let node_csr = crate::pair_ca::run(
            &[
                "req",
                "-new",
                "-key",
                &node_key_path.to_string_lossy(),
                "-subj",
                "/CN=node-1",
            ],
            None,
        )
        .unwrap();
        let node_cert =
            crate::fleet_ca::sign(&hub.ca_dir, "node-1", &String::from_utf8(node_csr).unwrap())
                .unwrap();
        let node_cert_path = hub.root.join("node.crt");
        crate::pair_ca::write_private(&node_cert_path, node_cert.as_bytes(), 0o644).unwrap();
        let node_der = crate::tls::certificates(&node_cert_path).unwrap()[0]
            .as_ref()
            .to_vec();
        assert!(crate::fleet_report::verify_hub_identity(&node_der, "node-1").is_err());
        let _ = std::fs::remove_dir_all(hub.root);
    }
}
