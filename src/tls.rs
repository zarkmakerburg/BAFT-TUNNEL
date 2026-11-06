use std::{fs::File, io::BufReader, path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    client::WebPkiServerVerifier,
    pki_types::{CertificateDer, CertificateRevocationListDer, PrivateKeyDer, ServerName},
    server::WebPkiClientVerifier,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let file =
        File::open(path).with_context(|| format!("open certificate file {}", path.display()))?;
    let items = rustls_pemfile::certs(&mut BufReader::new(file))
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parse PEM certificates")?;
    if items.is_empty() {
        bail!("certificate file contains no certificates");
    }
    Ok(items)
}

fn revocation_lists(path: &Path) -> Result<Vec<CertificateRevocationListDer<'static>>> {
    let file = File::open(path)
        .with_context(|| format!("open certificate revocation list {}", path.display()))?;
    let items = rustls_pemfile::crls(&mut BufReader::new(file))
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parse PEM certificate revocation lists")?;
    if items.is_empty() {
        bail!("certificate revocation list file contains no CRLs");
    }
    Ok(items)
}

fn private_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let file =
        File::open(path).with_context(|| format!("open private key file {}", path.display()))?;
    rustls_pemfile::private_key(&mut BufReader::new(file))
        .context("parse PEM private key")?
        .context("no supported private key found")
}

fn roots(path: &Path) -> Result<RootCertStore> {
    let mut store = RootCertStore::empty();
    for cert in certificates(path)? {
        store.add(cert).context("add CA certificate")?;
    }
    Ok(store)
}

pub fn client_config(ca: &Path, crl: &Path, cert: &Path, key: &Path) -> Result<Arc<ClientConfig>> {
    let verifier = WebPkiServerVerifier::builder(Arc::new(roots(ca)?))
        .with_crls(revocation_lists(crl)?)
        .enforce_revocation_expiration()
        .build()
        .context("build server certificate verifier with mandatory CRL checks")?;
    let mut config = ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .with_webpki_verifier(verifier)
        .with_client_auth_cert(certificates(cert)?, private_key(key)?)
        .context("build TLS 1.3 mutual-auth client configuration")?;
    // Forwarded application data is never sent as TLS 1.3 0-RTT early data.
    config.enable_early_data = false;
    Ok(Arc::new(config))
}

pub fn server_config(ca: &Path, crl: &Path, cert: &Path, key: &Path) -> Result<TlsAcceptor> {
    let verifier = WebPkiClientVerifier::builder(Arc::new(roots(ca)?))
        .with_crls(revocation_lists(crl)?)
        .enforce_revocation_expiration()
        .build()
        .context("build client certificate verifier with mandatory CRL checks")?;
    let mut config = ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .with_client_cert_verifier(verifier)
        .with_single_cert(certificates(cert)?, private_key(key)?)
        .context("build TLS 1.3 mutual-auth server configuration")?;
    // Keep server-side TLS 1.3 early data disabled for all tunnel traffic.
    config.max_early_data_size = 0;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub fn server_name(value: &str) -> Result<ServerName<'static>> {
    ServerName::try_from(value.to_owned()).context("invalid TLS server name")
}

pub fn connector(config: Arc<ClientConfig>) -> TlsConnector {
    TlsConnector::from(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handshake::{client_handshake, server_handshake, verify_peer_fingerprint};
    use rcgen::{
        BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
        KeyUsagePurpose, SanType, string::Ia5String,
    };
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        time::{Duration as StdDuration, SystemTime, UNIX_EPOCH},
    };
    use tokio::{io::duplex, time::timeout};

    #[tokio::test]
    async fn tls13_requires_mutual_certificates_and_pinned_peer_identity() {
        let directory = std::env::temp_dir().join(format!(
            "baft-tunnel-mtls-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();

        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        let ca_key = KeyPair::generate().unwrap();
        let ca_key_pem = ca_key.serialize_pem();
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();
        let issuer = Issuer::new(ca_params, ca_key);

        let mut server_params = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
        server_params
            .key_usages
            .push(KeyUsagePurpose::DigitalSignature);
        server_params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        server_params.use_authority_key_identifier_extension = true;
        server_params.subject_alt_names.push(SanType::URI(
            Ia5String::try_from("urn:baft-tunnel:node:abroad").unwrap(),
        ));
        let server_key = KeyPair::generate().unwrap();
        let server_cert = server_params.signed_by(&server_key, &issuer).unwrap();

        let mut client_params =
            CertificateParams::new(vec!["baft-test-client".to_owned()]).unwrap();
        client_params
            .key_usages
            .push(KeyUsagePurpose::DigitalSignature);
        client_params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ClientAuth,
            ExtendedKeyUsagePurpose::ServerAuth,
        ];
        client_params.use_authority_key_identifier_extension = true;
        client_params.subject_alt_names.push(SanType::URI(
            Ia5String::try_from("urn:baft-tunnel:node:iran").unwrap(),
        ));
        let client_key = KeyPair::generate().unwrap();
        let client_cert = client_params.signed_by(&client_key, &issuer).unwrap();

        let ca_path = directory.join("ca.pem");
        let crl_path = directory.join("ca.crl");
        let openssl_db = directory.join("index.txt");
        let new_certs = directory.join("newcerts");
        fs::create_dir_all(&new_certs).unwrap();
        fs::write(&openssl_db, "").unwrap();
        fs::write(directory.join("serial"), "1000\n").unwrap();
        fs::write(directory.join("crlnumber"), "1000\n").unwrap();
        let openssl_config = format!(
            "[ ca ]\ndefault_ca = CA_default\n[ CA_default ]\ndatabase = {}\nnew_certs_dir = {}\ncertificate = {}\nprivate_key = {}\nserial = {}\ncrlnumber = {}\ndefault_md = sha256\ndefault_crl_days = 2\npolicy = policy_any\n[ policy_any ]\ncommonName = supplied\n",
            openssl_db.display(),
            new_certs.display(),
            directory.join("ca.pem").display(),
            directory.join("ca.key").display(),
            directory.join("serial").display(),
            directory.join("crlnumber").display()
        );
        fs::write(directory.join("openssl.cnf"), openssl_config).unwrap();
        let server_cert_path = directory.join("server.pem");
        let server_key_path = directory.join("server.key");
        let client_cert_path = directory.join("client.pem");
        let client_key_path = directory.join("client.key");
        fs::write(&ca_path, ca_cert.pem()).unwrap();
        fs::write(directory.join("ca.key"), ca_key_pem).unwrap();
        let crl_status = std::process::Command::new("openssl")
            .args(["ca", "-gencrl", "-batch", "-config"])
            .arg(directory.join("openssl.cnf"))
            .arg("-out")
            .arg(&crl_path)
            .status()
            .expect("openssl is required for CRL integration tests");
        assert!(crl_status.success(), "openssl failed to generate test CRL");
        fs::write(&server_cert_path, server_cert.pem()).unwrap();
        fs::write(&server_key_path, server_key.serialize_pem()).unwrap();
        fs::write(&client_cert_path, client_cert.pem()).unwrap();
        fs::write(&client_key_path, client_key.serialize_pem()).unwrap();

        let server_pin = hex::encode(Sha256::digest(server_cert.der().as_ref()));
        let client_pin = hex::encode(Sha256::digest(client_cert.der().as_ref()));
        let acceptor =
            server_config(&ca_path, &crl_path, &server_cert_path, &server_key_path).unwrap();
        let client_tls_config =
            client_config(&ca_path, &crl_path, &client_cert_path, &client_key_path).unwrap();
        assert!(!client_tls_config.enable_early_data);
        let connector = connector(client_tls_config);

        let (client_io, server_io) = duplex(64 * 1024);
        let server_task = tokio::spawn(async move {
            let mut secure = timeout(StdDuration::from_secs(5), acceptor.accept(server_io))
                .await
                .unwrap()
                .unwrap();
            verify_peer_fingerprint(secure.get_ref().1.peer_certificates(), &client_pin).unwrap();
            crate::handshake::verify_peer_identity(secure.get_ref().1.peer_certificates(), "iran")
                .unwrap();
            assert!(
                crate::handshake::verify_peer_identity(
                    secure.get_ref().1.peer_certificates(),
                    "unexpected-node",
                )
                .is_err()
            );
            server_handshake(
                &mut secure,
                "abroad",
                "iran",
                "test-tunnel",
                crate::authorization::ConnectionDirection::Direct,
            )
            .await
            .unwrap();
        });

        let mut secure = timeout(
            StdDuration::from_secs(5),
            connector.connect(server_name("localhost").unwrap(), client_io),
        )
        .await
        .unwrap()
        .unwrap();
        verify_peer_fingerprint(secure.get_ref().1.peer_certificates(), &server_pin).unwrap();
        assert!(
            verify_peer_fingerprint(secure.get_ref().1.peer_certificates(), &"00".repeat(32),)
                .is_err()
        );
        assert!(
            crate::handshake::verify_peer_identity(
                secure.get_ref().1.peer_certificates(),
                "unexpected-node",
            )
            .is_err()
        );
        crate::handshake::verify_peer_identity(secure.get_ref().1.peer_certificates(), "abroad")
            .unwrap();
        client_handshake(
            &mut secure,
            "iran",
            "abroad",
            "test-tunnel",
            crate::authorization::ConnectionDirection::Direct,
        )
        .await
        .unwrap();
        server_task.await.unwrap();

        fs::remove_dir_all(directory).unwrap();
    }
}
