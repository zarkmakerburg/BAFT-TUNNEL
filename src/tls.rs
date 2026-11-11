use std::{fs::File, io::BufReader, path::Path, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    client::WebPkiServerVerifier,
    pki_types::{
        CertificateDer, CertificateRevocationListDer, PrivateKeyDer, ServerName, pem::PemObject,
    },
    server::WebPkiClientVerifier,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use zeroize::Zeroizing;

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

pub(crate) fn revocation_lists(path: &Path) -> Result<Vec<CertificateRevocationListDer<'static>>> {
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

/// Seconds until the earliest CRL `nextUpdate` in the file (negative once expired).
/// Operators use this to refresh the CRL before peers start failing closed.
pub fn crl_seconds_remaining(path: &Path) -> Result<i64> {
    use x509_parser::prelude::FromDer;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_secs() as i64;
    let mut earliest: Option<i64> = None;
    for der in revocation_lists(path)? {
        let (_, crl) =
            x509_parser::revocation_list::CertificateRevocationList::from_der(der.as_ref())
                .map_err(|error| anyhow::anyhow!("parse CRL: {error}"))?;
        let next = crl
            .next_update()
            .context("CRL has no nextUpdate; refusing to treat it as fresh")?
            .timestamp();
        earliest = Some(earliest.map_or(next, |current| current.min(next)));
    }
    Ok(earliest.context("certificate revocation list file contains no CRLs")? - now)
}

/// Verify the local leaf as both a TLS server and client using the exact trust/CRL policy
/// used by tunnel connections. This is used by the root enrollment helper before activation.
pub(crate) fn verify_local_certificate(
    ca: &Path,
    crl: &Path,
    cert: &Path,
    expected_server_name: &str,
) -> Result<()> {
    use rustls::{
        client::danger::ServerCertVerifier,
        pki_types::{ServerName, UnixTime},
    };
    let chain = certificates(cert)?;
    let leaf = chain
        .first()
        .context("certificate file contains no leaf certificate")?;
    let intermediates = &chain[1..];
    let roots = Arc::new(roots(ca)?);
    let revocation = revocation_lists(crl)?;
    let server_verifier = WebPkiServerVerifier::builder(roots.clone())
        .with_crls(revocation.clone())
        .enforce_revocation_expiration()
        .build()
        .context("build server certificate verifier for local enrollment")?;
    let server_name = if let Some(ip) = expected_server_name.strip_prefix("IP:") {
        ServerName::IpAddress(
            ip.parse::<std::net::IpAddr>()
                .context("configured server IP SAN is invalid")?
                .into(),
        )
    } else if let Some(dns) = expected_server_name.strip_prefix("DNS:") {
        ServerName::try_from(dns.to_owned()).context("configured DNS SAN is invalid")?
    } else {
        bail!("expected server SAN must be DNS or IP");
    };
    server_verifier
        .verify_server_cert(leaf, intermediates, &server_name, &[], UnixTime::now())
        .context("leaf certificate failed TLS server-auth, chain, validity or CRL verification")?;

    let client_verifier = WebPkiClientVerifier::builder(roots)
        .with_crls(revocation)
        .enforce_revocation_expiration()
        .build()
        .context("build client certificate verifier for local enrollment")?;
    client_verifier
        .verify_client_cert(leaf, intermediates, UnixTime::now())
        .context("leaf certificate failed TLS client-auth, chain, validity or CRL verification")?;
    Ok(())
}

/// Seconds until the first (leaf) certificate in the file expires (negative once expired).
pub fn certificate_seconds_remaining(path: &Path) -> Result<i64> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_secs() as i64;
    let certs = certificates(path)?;
    let leaf = certs
        .first()
        .context("certificate file contains no certificates")?;
    let (_, parsed) = x509_parser::parse_x509_certificate(leaf.as_ref())
        .map_err(|_| anyhow::anyhow!("certificate is not valid X.509 DER"))?;
    Ok(parsed.validity().not_after.timestamp() - now)
}

fn der_tlv<'a>(input: &'a [u8], offset: &mut usize) -> Result<(u8, &'a [u8])> {
    ensure!(*offset < input.len(), "truncated DER tag");
    let tag = input[*offset];
    *offset += 1;
    ensure!(*offset < input.len(), "truncated DER length");
    let first = input[*offset];
    *offset += 1;
    let length = if first & 0x80 == 0 {
        first as usize
    } else {
        let count = (first & 0x7f) as usize;
        ensure!(
            count > 0 && count <= 4 && *offset + count <= input.len(),
            "invalid DER length"
        );
        let mut value = 0usize;
        for byte in &input[*offset..*offset + count] {
            value = value
                .checked_mul(256)
                .and_then(|n| n.checked_add(*byte as usize))
                .context("DER length overflow")?;
        }
        *offset += count;
        value
    };
    let end = offset.checked_add(length).context("DER offset overflow")?;
    ensure!(end <= input.len(), "truncated DER value");
    let value = &input[*offset..end];
    *offset = end;
    Ok((tag, value))
}

fn der_encode(tag: u8, value: &[u8]) -> Result<Vec<u8>> {
    let mut output = vec![tag];
    if value.len() < 128 {
        output.push(value.len() as u8);
    } else {
        let bytes = value.len().to_be_bytes();
        let first = bytes
            .iter()
            .position(|byte| *byte != 0)
            .unwrap_or(bytes.len() - 1);
        let length_bytes = &bytes[first..];
        ensure!(length_bytes.len() <= 4, "DER value is too large");
        output.push(0x80 | length_bytes.len() as u8);
        output.extend_from_slice(length_bytes);
    }
    output.extend_from_slice(value);
    Ok(output)
}

/// Rewrite OpenSSL's explicit-parameter SEC1 output to named-curve SEC1 with the public point.
/// This keeps legacy P-256 PKCS#8 keys compatible with ring without writing the key to disk.
fn normalize_legacy_p256_sec1(explicit_der: &[u8]) -> Result<Vec<u8>> {
    let mut offset = 0usize;
    let (sequence_tag, sequence) = der_tlv(explicit_der, &mut offset)?;
    ensure!(
        sequence_tag == 0x30 && offset == explicit_der.len(),
        "invalid EC private-key DER sequence"
    );
    let mut inner = 0usize;
    let (version_tag, version) = der_tlv(sequence, &mut inner)?;
    ensure!(
        version_tag == 0x02 && version == [1],
        "unsupported EC private-key version"
    );
    let (private_tag, private_scalar) = der_tlv(sequence, &mut inner)?;
    ensure!(
        private_tag == 0x04 && (1..=32).contains(&private_scalar.len()),
        "legacy private key is not a P-256 scalar"
    );
    // OpenSSL's explicit-parameter SEC1 encoder can omit the leading zero octet
    // from the fixed-width P-256 scalar. Restore the required 32-byte width.
    let mut padded_scalar = Zeroizing::new([0u8; 32]);
    padded_scalar[32 - private_scalar.len()..].copy_from_slice(private_scalar);
    let (parameters_tag, _parameters) = der_tlv(sequence, &mut inner)?;
    ensure!(
        parameters_tag == 0xa0,
        "legacy EC key lacks explicit curve parameters"
    );
    let (public_tag, public_field) = der_tlv(sequence, &mut inner)?;
    ensure!(
        public_tag == 0xa1 && inner == sequence.len(),
        "legacy EC key lacks a final public-key field"
    );
    let mut public_offset = 0usize;
    let (bit_string_tag, public_bits) = der_tlv(public_field, &mut public_offset)?;
    ensure!(
        bit_string_tag == 0x03
            && public_offset == public_field.len()
            && public_bits.len() == 66
            && public_bits[0] == 0
            && public_bits[1] == 0x04,
        "legacy EC public point is not an uncompressed P-256 point"
    );

    const P256_NAMED_CURVE: &[u8] = &[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
    let parameters = der_encode(0xa0, P256_NAMED_CURVE)?;
    let public = der_encode(0xa1, &der_encode(0x03, public_bits)?)?;
    let private_tlv = Zeroizing::new(der_encode(0x04, padded_scalar.as_ref())?);
    let mut content = Zeroizing::new(Vec::with_capacity(128));
    content.extend_from_slice(&[0x02, 0x01, 0x01]);
    content.extend_from_slice(&private_tlv);
    content.extend_from_slice(&parameters);
    content.extend_from_slice(&public);
    der_encode(0x30, &content)
}

fn private_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let file =
        File::open(path).with_context(|| format!("open private key file {}", path.display()))?;
    let key =
        PrivateKeyDer::from_pem_reader(BufReader::new(file)).context("parse PEM private key")?;
    if matches!(&key, PrivateKeyDer::Pkcs8(_)) {
        // Older OpenSSL genpkey releases emitted PKCS#8 EC keys without the public-key field
        // required by ring's ECDSA parser. Normalize P-256 keys to SEC1 in memory only; never
        // write, log or export the private key. New enrollment writes SEC1 directly.
        let public = std::process::Command::new("/usr/bin/openssl")
            .args(["pkey", "-in"])
            .arg(path)
            .args(["-pubout", "-outform", "DER"])
            .output()
            .context("extract legacy EC public key for curve identification")?;
        if !public.status.success()
            || !crate::pki_helper::is_p256_subject_public_key_info(&public.stdout)
        {
            return Ok(key);
        }
        let explicit = std::process::Command::new("/usr/bin/openssl")
            .args(["ec", "-in"])
            .arg(path)
            .args(["-outform", "DER"])
            .output()
            .context("start fixed OpenSSL to read legacy P-256 private key")?;
        if !explicit.status.success() {
            bail!(
                "OpenSSL could not read the legacy P-256 private key: {}",
                String::from_utf8_lossy(&explicit.stderr).trim()
            );
        }
        let explicit_der = Zeroizing::new(explicit.stdout);
        let normalized = Zeroizing::new(normalize_legacy_p256_sec1(&explicit_der)?);
        return Ok(PrivateKeyDer::Sec1(
            rustls::pki_types::PrivateSec1KeyDer::from(normalized.as_slice().to_vec()),
        ));
    }
    Ok(key)
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

/// Setup helper: after checking the node identity URI SAN against `node_id`, return the
/// certificate's SHA-256 pin (hex) and its first DNS or IPv4 server name, if any.
pub fn node_certificate_summary(cert: &Path, node_id: &str) -> Result<(String, Option<String>)> {
    use sha2::{Digest, Sha256};
    use x509_parser::extensions::GeneralName;
    let certs = certificates(cert)?;
    crate::handshake::verify_peer_identity(Some(&certs), node_id)?;
    let leaf = certs
        .first()
        .context("certificate file contains no certificates")?;
    let pin = hex::encode(Sha256::digest(leaf.as_ref()));
    let (_, parsed) = x509_parser::parse_x509_certificate(leaf.as_ref())
        .map_err(|_| anyhow::anyhow!("node certificate is not valid X.509 DER"))?;
    let mut name = None;
    if let Some(san) = parsed
        .subject_alternative_name()
        .context("parse subject alternative name")?
    {
        for entry in &san.value.general_names {
            match entry {
                GeneralName::DNSName(dns) => {
                    name = Some((*dns).to_owned());
                    break;
                }
                GeneralName::IPAddress(bytes) if bytes.len() == 4 && name.is_none() => {
                    name = Some(format!(
                        "{}.{}.{}.{}",
                        bytes[0], bytes[1], bytes[2], bytes[3]
                    ));
                }
                _ => {}
            }
        }
    }
    Ok((pin, name))
}

pub fn server_name(value: &str) -> Result<ServerName<'static>> {
    ServerName::try_from(value.to_owned()).context("invalid TLS server name")
}

pub fn connector(config: Arc<ClientConfig>) -> TlsConnector {
    TlsConnector::from(config)
}

#[cfg(test)]
pub(crate) mod tests {
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

    #[test]
    fn normalizes_p256_scalar_when_openssl_omits_leading_zero_octet() {
        const P256_NAMED_CURVE: &[u8] =
            &[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
        let mut content = vec![0x02, 0x01, 0x01];
        content.extend_from_slice(&der_encode(0x04, &[1u8; 31]).unwrap());
        content.extend_from_slice(&der_encode(0xa0, P256_NAMED_CURVE).unwrap());
        let mut public_bits = vec![0u8, 0x04];
        public_bits.extend_from_slice(&[2u8; 64]);
        let bit_string = der_encode(0x03, &public_bits).unwrap();
        content.extend_from_slice(&der_encode(0xa1, &bit_string).unwrap());
        let explicit = der_encode(0x30, &content).unwrap();

        let normalized = normalize_legacy_p256_sec1(&explicit).unwrap();
        let mut offset = 0;
        let (_, sequence) = der_tlv(&normalized, &mut offset).unwrap();
        let mut inner = 0;
        let _ = der_tlv(sequence, &mut inner).unwrap();
        let (_, scalar) = der_tlv(sequence, &mut inner).unwrap();
        assert_eq!(scalar.len(), 32);
        assert_eq!(scalar[0], 0);
        assert_eq!(&scalar[1..], &[1u8; 31]);
    }

    #[test]
    fn legacy_openssl_pkcs8_ec_keys_are_normalized_for_rustls_ring_in_memory() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "baft-legacy-pkcs8-{}-{}.pem",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let status = std::process::Command::new("/usr/bin/openssl")
            .args([
                "genpkey",
                "-algorithm",
                "EC",
                "-pkeyopt",
                "ec_paramgen_curve:P-256",
                "-out",
            ])
            .arg(&path)
            .status()
            .expect("openssl is required for legacy PKCS#8 compatibility test");
        assert!(status.success());
        let original = std::fs::read_to_string(&path).unwrap();
        assert!(original.starts_with("-----BEGIN PRIVATE KEY-----"));
        let normalized = private_key(&path).unwrap();
        assert!(matches!(normalized, PrivateKeyDer::Sec1(_)));
        assert!(rustls::crypto::ring::sign::any_supported_type(&normalized).is_ok());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let _ = std::fs::remove_file(path);
    }

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
            "[ ca ]\ndefault_ca = CA_default\n[ CA_default ]\ndatabase = {}\nnew_certs_dir = {}\ncertificate = {}\nprivate_key = {}\nserial = {}\ncrlnumber = {}\ncrl_extensions = crl_ext\ndefault_md = sha256\ndefault_crl_days = 2\npolicy = policy_any\n[ crl_ext ]\nauthorityKeyIdentifier = keyid:always\n[ policy_any ]\ncommonName = supplied\n",
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
        // Rollover window: the real pin is accepted in either position next to a stale one,
        // while two wrong pins and three pins are rejected.
        let stale = "00".repeat(32);
        let certs = secure.get_ref().1.peer_certificates();
        verify_peer_fingerprint(certs, &format!("{stale},{server_pin}")).unwrap();
        verify_peer_fingerprint(certs, &format!("{server_pin},{stale}")).unwrap();
        assert!(verify_peer_fingerprint(certs, &format!("{stale},{}", "11".repeat(32))).is_err());
        assert!(
            verify_peer_fingerprint(certs, &format!("{stale},{},{server_pin}", "11".repeat(32)))
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
            "[ ca ]\ndefault_ca = CA_default\n[ CA_default ]\ndatabase = {}\nnew_certs_dir = {}\ncertificate = {}\nprivate_key = {}\nserial = {}\ncrlnumber = {}\ncrl_extensions = crl_ext\ndefault_md = sha256\ndefault_crl_days = 2\npolicy = policy_any\n[ crl_ext ]\nauthorityKeyIdentifier = keyid:always\n[ policy_any ]\ncommonName = supplied\n",
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

    pub(crate) struct TestCa {
        pub(crate) ca_pem: std::path::PathBuf,
        pub(crate) crl_pem: std::path::PathBuf,
        pub(crate) issuer: Issuer<'static, KeyPair>,
    }

    pub(crate) fn make_ca(dir: &Path, name: &str) -> TestCa {
        let dir = dir.join(name);
        fs::create_dir_all(dir.join("newcerts")).unwrap();
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        // Distinct subjects: CRLs are matched to their issuing CA by name.
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, format!("baft-test-ca-{name}"));
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        let key = KeyPair::generate().unwrap();
        let key_pem = key.serialize_pem();
        let cert = params.self_signed(&key).unwrap();
        let issuer = Issuer::new(params, key);
        let (ca_pem, ca_key, crl_pem) =
            (dir.join("ca.pem"), dir.join("ca.key"), dir.join("ca.crl"));
        fs::write(&ca_pem, cert.pem()).unwrap();
        fs::write(&ca_key, key_pem).unwrap();
        fs::write(dir.join("index.txt"), "").unwrap();
        fs::write(dir.join("serial"), "1000\n").unwrap();
        fs::write(dir.join("crlnumber"), "1000\n").unwrap();
        let config = format!(
            "[ ca ]\ndefault_ca = CA_default\n[ CA_default ]\ndatabase = {}\nnew_certs_dir = {}\ncertificate = {}\nprivate_key = {}\nserial = {}\ncrlnumber = {}\ncrl_extensions = crl_ext\ndefault_md = sha256\ndefault_crl_days = 2\npolicy = policy_any\n[ crl_ext ]\nauthorityKeyIdentifier = keyid:always\n[ policy_any ]\ncommonName = supplied\n",
            dir.join("index.txt").display(),
            dir.join("newcerts").display(),
            ca_pem.display(),
            ca_key.display(),
            dir.join("serial").display(),
            dir.join("crlnumber").display()
        );
        fs::write(dir.join("openssl.cnf"), config).unwrap();
        let status = std::process::Command::new("openssl")
            .args(["ca", "-gencrl", "-batch", "-config"])
            .arg(dir.join("openssl.cnf"))
            .arg("-out")
            .arg(&crl_pem)
            .status()
            .expect("openssl is required for CRL integration tests");
        assert!(status.success(), "openssl failed to generate test CRL");
        TestCa {
            ca_pem,
            crl_pem,
            issuer,
        }
    }

    pub(crate) fn leaf(
        dir: &Path,
        name: &str,
        uri: &str,
        issuer: &Issuer<'static, KeyPair>,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let mut params = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
        params.key_usages.push(KeyUsagePurpose::DigitalSignature);
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        params.use_authority_key_identifier_extension = true;
        params
            .subject_alt_names
            .push(SanType::URI(Ia5String::try_from(uri.to_owned()).unwrap()));
        let key = KeyPair::generate().unwrap();
        let cert = params.signed_by(&key, issuer).unwrap();
        let (cert_path, key_path) = (
            dir.join(format!("{name}.pem")),
            dir.join(format!("{name}.key")),
        );
        fs::write(&cert_path, cert.pem()).unwrap();
        fs::write(&key_path, key.serialize_pem()).unwrap();
        (cert_path, key_path)
    }

    async fn mtls_handshake_succeeds(
        acceptor: TlsAcceptor,
        client_config: Arc<ClientConfig>,
    ) -> bool {
        let (client_io, server_io) = duplex(64 * 1024);
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let Ok(mut stream) = acceptor.accept(server_io).await else {
                return false;
            };
            let mut byte = [0_u8; 1];
            stream.read_exact(&mut byte).await.is_ok() && stream.write_all(&byte).await.is_ok()
        });
        let client = timeout(
            StdDuration::from_secs(5),
            connector(client_config).connect(server_name("localhost").unwrap(), client_io),
        )
        .await;
        let mut client_ok = false;
        if let Ok(Ok(mut stream)) = client {
            // TLS 1.3 client-auth failures surface on first use, so exchange one byte.
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut byte = [0_u8; 1];
            client_ok = stream.write_all(b"x").await.is_ok()
                && matches!(
                    timeout(StdDuration::from_secs(2), stream.read_exact(&mut byte)).await,
                    Ok(Ok(_))
                );
        }
        let server_ok = timeout(StdDuration::from_secs(5), server)
            .await
            .map(|joined| joined.unwrap_or(false))
            .unwrap_or(false);
        client_ok && server_ok
    }

    #[tokio::test]
    async fn ca_bundle_with_per_ca_crls_supports_rollover_overlap() {
        let dir = std::env::temp_dir().join(format!(
            "baft-tunnel-dual-ca-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let old = make_ca(&dir, "old");
        let new = make_ca(&dir, "new");
        let unknown = make_ca(&dir, "unknown");

        // Overlap window: trust bundle holds both CAs and one CRL per CA.
        let bundle_ca = dir.join("bundle-ca.pem");
        let bundle_crl = dir.join("bundle.crl");
        let cat = |paths: &[&Path], out: &Path| {
            let mut text = String::new();
            for path in paths {
                text.push_str(&fs::read_to_string(path).unwrap());
            }
            fs::write(out, text).unwrap();
        };
        cat(&[&old.ca_pem, &new.ca_pem], &bundle_ca);
        cat(&[&old.crl_pem, &new.crl_pem], &bundle_crl);

        let (server_cert, server_key) =
            leaf(&dir, "server", "urn:baft-tunnel:node:abroad", &old.issuer);
        let accept_with =
            |crl: &Path| server_config(&bundle_ca, crl, &server_cert, &server_key).unwrap();
        let client_for = |ca: &TestCa, name: &str, trust_crl: &Path| {
            let (cert, key) = leaf(&dir, name, "urn:baft-tunnel:node:iran", &ca.issuer);
            client_config(&bundle_ca, trust_crl, &cert, &key).unwrap()
        };

        // Old-CA and new-CA client certificates both authenticate during the overlap.
        assert!(
            mtls_handshake_succeeds(
                accept_with(&bundle_crl),
                client_for(&old, "old-client", &bundle_crl)
            )
            .await
        );
        assert!(
            mtls_handshake_succeeds(
                accept_with(&bundle_crl),
                client_for(&new, "new-client", &bundle_crl)
            )
            .await
        );

        // A CA outside the bundle is rejected.
        assert!(
            !mtls_handshake_succeeds(
                accept_with(&bundle_crl),
                client_for(&unknown, "unknown-client", &bundle_crl)
            )
            .await
        );

        // Fail closed: with only the old CA's CRL loaded, new-CA certificates have unknown
        // revocation status and are rejected until the new CRL is activated.
        let old_crl_only = dir.join("old-only.crl");
        cat(&[&old.crl_pem], &old_crl_only);
        assert!(
            !mtls_handshake_succeeds(
                accept_with(&old_crl_only),
                client_for(&new, "new-client-2", &bundle_crl)
            )
            .await
        );
        assert!(
            mtls_handshake_succeeds(
                accept_with(&old_crl_only),
                client_for(&old, "old-client-2", &bundle_crl)
            )
            .await
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
