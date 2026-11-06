use std::{fs::File, io::BufReader, path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    client::WebPkiServerVerifier,
    pki_types::{
        CertificateDer, CertificateRevocationListDer, PrivateKeyDer, ServerName, pem::PemObject,
    },
    server::WebPkiClientVerifier,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let file =
        File::open(path).with_context(|| format!("open certificate file {}", path.display()))?;
    let items = CertificateDer::pem_reader_iter(BufReader::new(file))
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
    let items = CertificateRevocationListDer::pem_reader_iter(BufReader::new(file))
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
    PrivateKeyDer::from_pem_reader(BufReader::new(file)).context("parse PEM private key")
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
    async fn tls13_enforces_mtls_pins_identity_and_certificate_validity() {
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

        // Generate a syntactically valid CRL whose validity window is long expired.
        // Verify expiration enforcement independently on both TLS roles below.
        let expired_crl_path = directory.join("expired-ca.crl");
        let expired_crl_status = std::process::Command::new("openssl")
            .args(["ca", "-gencrl", "-batch", "-config"])
            .arg(directory.join("openssl.cnf"))
            .args([
                "-crl_lastupdate",
                "20200101000000Z",
                "-crl_nextupdate",
                "20200102000000Z",
                "-out",
            ])
            .arg(&expired_crl_path)
            .status()
            .expect("openssl is required for expired-CRL integration tests");
        assert!(
            expired_crl_status.success(),
            "openssl failed to generate expired test CRL"
        );

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
        let initial_connector = connector(client_tls_config);

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
            initial_connector.connect(server_name("localhost").unwrap(), client_io),
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

        // An expired CRL must fail closed when verifying a client certificate (server role).
        let expired_crl_acceptor = server_config(
            &ca_path,
            &expired_crl_path,
            &server_cert_path,
            &server_key_path,
        )
        .unwrap();
        let current_crl_connector = connector(
            client_config(&ca_path, &crl_path, &client_cert_path, &client_key_path).unwrap(),
        );
        let (expired_server_client_io, expired_server_io) = duplex(64 * 1024);
        let expired_server_task = tokio::spawn(async move {
            timeout(
                StdDuration::from_secs(5),
                expired_crl_acceptor.accept(expired_server_io),
            )
            .await
        });
        let expired_server_client_result = timeout(
            StdDuration::from_secs(5),
            current_crl_connector
                .connect(server_name("localhost").unwrap(), expired_server_client_io),
        )
        .await;
        let expired_server_result = timeout(StdDuration::from_secs(5), expired_server_task)
            .await
            .unwrap()
            .unwrap();
        let expired_server_client_failed = expired_server_client_result
            .as_ref()
            .map_or(true, |handshake| handshake.is_err());
        let expired_server_failed = expired_server_result
            .as_ref()
            .map_or(true, |handshake| handshake.is_err());
        assert!(
            expired_server_client_failed || expired_server_failed,
            "server must reject client authentication when its configured CRL is expired"
        );

        // An expired CRL must also fail closed when verifying the server certificate (client role).
        let current_crl_acceptor =
            server_config(&ca_path, &crl_path, &server_cert_path, &server_key_path).unwrap();
        let expired_crl_connector = connector(
            client_config(
                &ca_path,
                &expired_crl_path,
                &client_cert_path,
                &client_key_path,
            )
            .unwrap(),
        );
        let (expired_client_io, expired_client_server_io) = duplex(64 * 1024);
        let expired_client_server_task = tokio::spawn(async move {
            timeout(
                StdDuration::from_secs(5),
                current_crl_acceptor.accept(expired_client_server_io),
            )
            .await
        });
        let expired_client_result = timeout(
            StdDuration::from_secs(5),
            expired_crl_connector.connect(server_name("localhost").unwrap(), expired_client_io),
        )
        .await;
        let expired_client_server_result =
            timeout(StdDuration::from_secs(5), expired_client_server_task)
                .await
                .unwrap()
                .unwrap();
        let expired_client_failed = expired_client_result
            .as_ref()
            .map_or(true, |handshake| handshake.is_err());
        let expired_client_server_failed = expired_client_server_result
            .as_ref()
            .map_or(true, |handshake| handshake.is_err());
        assert!(
            expired_client_failed || expired_client_server_failed,
            "client must reject server authentication when its configured CRL is expired"
        );

        // A certificate signed by a different CA must not pass client authentication.
        let mut rogue_ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        rogue_ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        rogue_ca_params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        let rogue_ca_key = KeyPair::generate().unwrap();
        let rogue_ca_key_pem = rogue_ca_key.serialize_pem();
        let rogue_ca_cert = rogue_ca_params.self_signed(&rogue_ca_key).unwrap();
        let rogue_issuer = Issuer::new(rogue_ca_params, rogue_ca_key);

        let rogue_pki_dir = directory.join("rogue-pki");
        let rogue_new_certs = rogue_pki_dir.join("newcerts");
        fs::create_dir_all(&rogue_new_certs).unwrap();
        let rogue_ca_path = rogue_pki_dir.join("ca.pem");
        let rogue_ca_key_path = rogue_pki_dir.join("ca.key");
        let rogue_crl_path = rogue_pki_dir.join("ca.crl");
        fs::write(&rogue_ca_path, rogue_ca_cert.pem()).unwrap();
        fs::write(&rogue_ca_key_path, rogue_ca_key_pem).unwrap();
        fs::write(rogue_pki_dir.join("index.txt"), "").unwrap();
        fs::write(rogue_pki_dir.join("serial"), "1000\n").unwrap();
        fs::write(rogue_pki_dir.join("crlnumber"), "1000\n").unwrap();
        let rogue_openssl_config = format!(
            "[ ca ]\ndefault_ca = CA_default\n[ CA_default ]\ndatabase = {}\nnew_certs_dir = {}\ncertificate = {}\nprivate_key = {}\nserial = {}\ncrlnumber = {}\ndefault_md = sha256\ndefault_crl_days = 2\npolicy = policy_any\n[ policy_any ]\ncommonName = supplied\n",
            rogue_pki_dir.join("index.txt").display(),
            rogue_new_certs.display(),
            rogue_ca_path.display(),
            rogue_ca_key_path.display(),
            rogue_pki_dir.join("serial").display(),
            rogue_pki_dir.join("crlnumber").display()
        );
        fs::write(rogue_pki_dir.join("openssl.cnf"), rogue_openssl_config).unwrap();
        let rogue_crl_status = std::process::Command::new("openssl")
            .args(["ca", "-gencrl", "-batch", "-config"])
            .arg(rogue_pki_dir.join("openssl.cnf"))
            .arg("-out")
            .arg(&rogue_crl_path)
            .status()
            .expect("openssl is required for CRL issuer-mismatch tests");
        assert!(
            rogue_crl_status.success(),
            "openssl failed to generate rogue CRL"
        );

        // A parseable CRL from the wrong issuer must not turn unknown status into trust.
        let wrong_crl_acceptor = server_config(
            &ca_path,
            &rogue_crl_path,
            &server_cert_path,
            &server_key_path,
        )
        .unwrap();
        let valid_crl_connector = connector(
            client_config(&ca_path, &crl_path, &client_cert_path, &client_key_path).unwrap(),
        );
        let (wrong_crl_client_io, wrong_crl_server_io) = duplex(64 * 1024);
        let wrong_crl_server_task =
            tokio::spawn(async move { wrong_crl_acceptor.accept(wrong_crl_server_io).await });
        let wrong_crl_client_result = timeout(
            StdDuration::from_secs(5),
            valid_crl_connector.connect(server_name("localhost").unwrap(), wrong_crl_client_io),
        )
        .await;
        let wrong_crl_server_result = timeout(StdDuration::from_secs(5), wrong_crl_server_task)
            .await
            .unwrap()
            .unwrap();
        assert!(
            wrong_crl_client_result.is_err() || wrong_crl_server_result.is_err(),
            "a CRL signed by the wrong issuer must fail closed on the server"
        );

        let valid_crl_acceptor =
            server_config(&ca_path, &crl_path, &server_cert_path, &server_key_path).unwrap();
        let wrong_crl_connector = connector(
            client_config(
                &ca_path,
                &rogue_crl_path,
                &client_cert_path,
                &client_key_path,
            )
            .unwrap(),
        );
        let (wrong_crl_server_client_io, wrong_crl_server_io) = duplex(64 * 1024);
        let wrong_crl_client_server_task =
            tokio::spawn(async move { valid_crl_acceptor.accept(wrong_crl_server_io).await });
        let wrong_crl_server_client_result = timeout(
            StdDuration::from_secs(5),
            wrong_crl_connector.connect(
                server_name("localhost").unwrap(),
                wrong_crl_server_client_io,
            ),
        )
        .await
        .unwrap();
        assert!(
            wrong_crl_server_client_result.is_err(),
            "a CRL signed by the wrong issuer must fail closed on the client"
        );
        let _ = timeout(StdDuration::from_secs(5), wrong_crl_client_server_task).await;

        let mut rogue_client_params =
            CertificateParams::new(vec!["baft-rogue-client".to_owned()]).unwrap();
        rogue_client_params
            .key_usages
            .push(KeyUsagePurpose::DigitalSignature);
        rogue_client_params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ClientAuth,
            ExtendedKeyUsagePurpose::ServerAuth,
        ];
        rogue_client_params.subject_alt_names.push(SanType::URI(
            Ia5String::try_from("urn:baft-tunnel:node:iran").unwrap(),
        ));
        let rogue_client_key = KeyPair::generate().unwrap();
        let rogue_client_cert = rogue_client_params
            .signed_by(&rogue_client_key, &rogue_issuer)
            .unwrap();
        let rogue_client_cert_path = directory.join("rogue-client.pem");
        let rogue_client_key_path = directory.join("rogue-client.key");
        fs::write(&rogue_client_cert_path, rogue_client_cert.pem()).unwrap();
        fs::write(&rogue_client_key_path, rogue_client_key.serialize_pem()).unwrap();
        let trusted_acceptor =
            server_config(&ca_path, &crl_path, &server_cert_path, &server_key_path).unwrap();
        let rogue_connector = connector(
            client_config(
                &ca_path,
                &crl_path,
                &rogue_client_cert_path,
                &rogue_client_key_path,
            )
            .unwrap(),
        );
        let (rogue_client_io, rogue_server_io) = duplex(64 * 1024);
        let rogue_server_task =
            tokio::spawn(async move { trusted_acceptor.accept(rogue_server_io).await });
        let rogue_client_result = timeout(
            StdDuration::from_secs(5),
            rogue_connector.connect(server_name("localhost").unwrap(), rogue_client_io),
        )
        .await;
        let rogue_server_result = timeout(StdDuration::from_secs(5), rogue_server_task)
            .await
            .unwrap()
            .unwrap();
        assert!(
            rogue_client_result.is_err() || rogue_server_result.is_err(),
            "an untrusted client certificate must be rejected"
        );

        // A CA-signed but expired client certificate must be rejected by the server.
        let mut expired_client_params =
            CertificateParams::new(vec!["baft-expired-client".to_owned()]).unwrap();
        expired_client_params.not_before = rcgen::date_time_ymd(2020, 1, 1);
        expired_client_params.not_after = rcgen::date_time_ymd(2020, 1, 2);
        expired_client_params
            .key_usages
            .push(KeyUsagePurpose::DigitalSignature);
        expired_client_params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ClientAuth,
            ExtendedKeyUsagePurpose::ServerAuth,
        ];
        expired_client_params.subject_alt_names.push(SanType::URI(
            Ia5String::try_from("urn:baft-tunnel:node:iran").unwrap(),
        ));
        let expired_client_key = KeyPair::generate().unwrap();
        let expired_client_cert = expired_client_params
            .signed_by(&expired_client_key, &issuer)
            .unwrap();
        let expired_client_cert_path = directory.join("expired-client.pem");
        let expired_client_key_path = directory.join("expired-client.key");
        fs::write(&expired_client_cert_path, expired_client_cert.pem()).unwrap();
        fs::write(&expired_client_key_path, expired_client_key.serialize_pem()).unwrap();
        let expired_client_acceptor =
            server_config(&ca_path, &crl_path, &server_cert_path, &server_key_path).unwrap();
        let expired_client_connector = connector(
            client_config(
                &ca_path,
                &crl_path,
                &expired_client_cert_path,
                &expired_client_key_path,
            )
            .unwrap(),
        );
        let (expired_client_io, expired_server_io) = duplex(64 * 1024);
        let expired_server_task =
            tokio::spawn(async move { expired_client_acceptor.accept(expired_server_io).await });
        let expired_client_result = timeout(
            StdDuration::from_secs(5),
            expired_client_connector.connect(server_name("localhost").unwrap(), expired_client_io),
        )
        .await;
        let expired_server_result = timeout(StdDuration::from_secs(5), expired_server_task)
            .await
            .unwrap()
            .unwrap();
        assert!(
            expired_client_result.is_err() || expired_server_result.is_err(),
            "an expired client certificate must be rejected"
        );

        // The client must reject both expired and untrusted server certificates.
        let mut expired_server_params =
            CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
        expired_server_params.not_before = rcgen::date_time_ymd(2020, 1, 1);
        expired_server_params.not_after = rcgen::date_time_ymd(2020, 1, 2);
        expired_server_params
            .key_usages
            .push(KeyUsagePurpose::DigitalSignature);
        expired_server_params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        expired_server_params.subject_alt_names.push(SanType::URI(
            Ia5String::try_from("urn:baft-tunnel:node:abroad").unwrap(),
        ));
        let expired_server_key = KeyPair::generate().unwrap();
        let expired_server_cert = expired_server_params
            .signed_by(&expired_server_key, &issuer)
            .unwrap();
        let expired_server_cert_path = directory.join("expired-server.pem");
        let expired_server_key_path = directory.join("expired-server.key");
        fs::write(&expired_server_cert_path, expired_server_cert.pem()).unwrap();
        fs::write(&expired_server_key_path, expired_server_key.serialize_pem()).unwrap();
        let expired_acceptor = server_config(
            &ca_path,
            &crl_path,
            &expired_server_cert_path,
            &expired_server_key_path,
        )
        .unwrap();
        let trusted_connector = connector(
            client_config(&ca_path, &crl_path, &client_cert_path, &client_key_path).unwrap(),
        );
        let (expired_server_client_io, expired_server_io) = duplex(64 * 1024);
        let expired_server_task =
            tokio::spawn(async move { expired_acceptor.accept(expired_server_io).await });
        let expired_server_result = timeout(
            StdDuration::from_secs(5),
            trusted_connector.connect(server_name("localhost").unwrap(), expired_server_client_io),
        )
        .await
        .unwrap();
        assert!(
            expired_server_result.is_err(),
            "an expired server certificate must be rejected by the client"
        );
        let _ = timeout(StdDuration::from_secs(5), expired_server_task).await;

        let rogue_server_params = {
            let mut params = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
            params.key_usages.push(KeyUsagePurpose::DigitalSignature);
            params.extended_key_usages = vec![
                ExtendedKeyUsagePurpose::ServerAuth,
                ExtendedKeyUsagePurpose::ClientAuth,
            ];
            params.subject_alt_names.push(SanType::URI(
                Ia5String::try_from("urn:baft-tunnel:node:abroad").unwrap(),
            ));
            params
        };
        let rogue_server_key = KeyPair::generate().unwrap();
        let rogue_server_cert = rogue_server_params
            .signed_by(&rogue_server_key, &rogue_issuer)
            .unwrap();
        let rogue_server_cert_path = directory.join("rogue-server.pem");
        let rogue_server_key_path = directory.join("rogue-server.key");
        fs::write(&rogue_server_cert_path, rogue_server_cert.pem()).unwrap();
        fs::write(&rogue_server_key_path, rogue_server_key.serialize_pem()).unwrap();
        let rogue_server_acceptor = server_config(
            &ca_path,
            &crl_path,
            &rogue_server_cert_path,
            &rogue_server_key_path,
        )
        .unwrap();
        let rogue_server_connector = connector(
            client_config(&ca_path, &crl_path, &client_cert_path, &client_key_path).unwrap(),
        );
        let (rogue_server_client_io, rogue_server_io) = duplex(64 * 1024);
        let rogue_server_task =
            tokio::spawn(async move { rogue_server_acceptor.accept(rogue_server_io).await });
        let rogue_server_result = timeout(
            StdDuration::from_secs(5),
            rogue_server_connector
                .connect(server_name("localhost").unwrap(), rogue_server_client_io),
        )
        .await
        .unwrap();
        assert!(
            rogue_server_result.is_err(),
            "an untrusted server certificate must be rejected by the client"
        );
        let _ = timeout(StdDuration::from_secs(5), rogue_server_task).await;

        let missing_crl = directory.join("missing.crl");
        assert!(
            client_config(&ca_path, &missing_crl, &client_cert_path, &client_key_path).is_err()
        );
        let malformed_crl = directory.join("malformed.crl");
        fs::write(&malformed_crl, "not a PEM CRL").unwrap();
        assert!(
            server_config(
                &ca_path,
                &malformed_crl,
                &server_cert_path,
                &server_key_path,
            )
            .is_err()
        );

        fs::remove_dir_all(directory).unwrap();
    }
}
