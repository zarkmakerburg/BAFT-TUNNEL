use anyhow::{Context, Result, bail};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const VERSION: u16 = 1;
const MAX_MESSAGE: usize = 2048;
const NONCE_BYTES: usize = 32;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientHello {
    version: u16,
    client_id: String,
    server_id: String,
    tunnel_id: String,
    nonce: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerHello {
    version: u16,
    server_id: String,
    client_id: String,
    tunnel_id: String,
    client_nonce: String,
    server_nonce: String,
}

pub fn validate_identity(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        bail!("identity must be 1-64 ASCII letters, digits, '.', '_' or '-'");
    }
    Ok(())
}

pub fn validate_fingerprint(value: &str) -> Result<()> {
    let normalized = value.replace(':', "");
    if normalized.len() != 64 || !normalized.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("peer fingerprint must be a SHA-256 hex digest (64 hex characters)");
    }
    Ok(())
}

fn nonce() -> String {
    let mut bytes = [0_u8; NONCE_BYTES];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn valid_nonce(value: &str) -> bool {
    value.len() == NONCE_BYTES * 2 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

async fn write_message<S: AsyncWrite + Unpin, T: Serialize>(
    stream: &mut S,
    value: &T,
) -> Result<()> {
    let bytes = serde_json::to_vec(value).context("encode handshake")?;
    if bytes.is_empty() || bytes.len() > MAX_MESSAGE {
        bail!("handshake message exceeds size limit");
    }
    let length = u16::try_from(bytes.len()).context("handshake length overflow")?;
    stream.write_all(&length.to_be_bytes()).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_message<S: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(
    stream: &mut S,
) -> Result<T> {
    let mut prefix = [0_u8; 2];
    stream.read_exact(&mut prefix).await?;
    let length = u16::from_be_bytes(prefix) as usize;
    if length == 0 || length > MAX_MESSAGE {
        bail!("invalid handshake frame length");
    }
    let mut bytes = vec![0_u8; length];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).context("decode handshake")
}

pub async fn client_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    local: &str,
    peer: &str,
    tunnel: &str,
) -> Result<()> {
    let client_nonce = nonce();
    write_message(
        stream,
        &ClientHello {
            version: VERSION,
            client_id: local.into(),
            server_id: peer.into(),
            tunnel_id: tunnel.into(),
            nonce: client_nonce.clone(),
        },
    )
    .await?;
    let response: ServerHello = read_message(stream).await?;
    if response.version != VERSION
        || response.server_id != peer
        || response.client_id != local
        || response.tunnel_id != tunnel
        || response.client_nonce != client_nonce
        || !valid_nonce(&response.server_nonce)
        || response.server_nonce == client_nonce
    {
        bail!("peer rejected or returned an invalid protocol handshake");
    }
    Ok(())
}

pub async fn server_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    local: &str,
    peer: &str,
    tunnel: &str,
) -> Result<()> {
    let request: ClientHello = read_message(stream).await?;
    if request.version != VERSION
        || request.client_id != peer
        || request.server_id != local
        || request.tunnel_id != tunnel
        || !valid_nonce(&request.nonce)
    {
        bail!("peer rejected or returned an invalid protocol handshake");
    }
    write_message(
        stream,
        &ServerHello {
            version: VERSION,
            server_id: local.into(),
            client_id: peer.into(),
            tunnel_id: tunnel.into(),
            client_nonce: request.nonce,
            server_nonce: nonce(),
        },
    )
    .await
}

/// Bind the TLS-authenticated certificate to the enrolled logical node identity.
/// The exact URI SAN contract is `urn:baft-tunnel:node:<node-id>`.
pub fn verify_peer_identity(
    certs: Option<&[rustls::pki_types::CertificateDer<'_>]>,
    expected_peer_id: &str,
) -> Result<()> {
    validate_identity(expected_peer_id)?;
    let leaf = certs
        .and_then(|items| items.first())
        .context("TLS peer did not present a certificate")?;
    let (_, certificate) = x509_parser::parse_x509_certificate(leaf.as_ref())
        .map_err(|_| anyhow::anyhow!("TLS peer certificate is not valid X.509 DER"))?;
    let expected_uri = format!("urn:baft-tunnel:node:{expected_peer_id}");
    let mut matched = false;
    for extension in certificate.extensions() {
        if let x509_parser::extensions::ParsedExtension::SubjectAlternativeName(san) =
            extension.parsed_extension()
        {
            for name in &san.general_names {
                if let x509_parser::extensions::GeneralName::URI(uri) = name {
                    if *uri == expected_uri {
                        matched = true;
                    }
                }
            }
        }
    }
    if !matched {
        bail!("TLS certificate URI SAN does not match the enrolled peer identity");
    }
    Ok(())
}

pub fn verify_peer_fingerprint(
    certs: Option<&[rustls::pki_types::CertificateDer<'_>]>,
    expected: &str,
) -> Result<()> {
    let leaf = certs
        .and_then(|items| items.first())
        .context("TLS peer did not present a certificate")?;
    let actual = hex::encode(Sha256::digest(leaf.as_ref()));
    if actual != expected.replace(':', "").to_ascii_lowercase() {
        bail!("TLS peer certificate fingerprint does not match the configured pin");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[test]
    fn identity_input_is_bounded_and_ascii() {
        assert!(validate_identity("node-iran_1.example").is_ok());
        assert!(validate_identity("").is_err());
        assert!(validate_identity("bad/id").is_err());
        assert!(validate_identity(&"x".repeat(65)).is_err());
    }

    #[test]
    fn fingerprint_requires_sha256_hex() {
        assert!(validate_fingerprint(&"ab".repeat(32)).is_ok());
        assert!(validate_fingerprint(&("AB:".to_owned() + &"CD:".repeat(30) + "EF")).is_ok());
        assert!(validate_fingerprint("not-a-fingerprint").is_err());
        assert!(validate_fingerprint(&"0".repeat(63)).is_err());
    }

    #[test]
    fn certificate_uri_san_must_match_enrolled_node_identity() {
        use rcgen::{CertificateParams, KeyPair, SanType, string::Ia5String};

        let mut params = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
        params.subject_alt_names.push(SanType::URI(
            Ia5String::try_from("urn:baft-tunnel:node:peer-1").unwrap(),
        ));
        let key = KeyPair::generate().unwrap();
        let certificate = params.self_signed(&key).unwrap();
        let der = rustls::pki_types::CertificateDer::from(certificate.der().to_vec());
        let chain = [der];

        assert!(verify_peer_identity(Some(&chain), "peer-1").is_ok());
        assert!(verify_peer_identity(Some(&chain), "peer-2").is_err());
        assert!(verify_peer_identity(Some(&chain), "bad/id").is_err());
        assert!(verify_peer_identity(None, "peer-1").is_err());
    }

    #[test]
    fn certificate_without_node_uri_san_is_rejected() {
        use rcgen::{CertificateParams, KeyPair};

        let params = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
        let key = KeyPair::generate().unwrap();
        let certificate = params.self_signed(&key).unwrap();
        let der = rustls::pki_types::CertificateDer::from(certificate.der().to_vec());
        assert!(verify_peer_identity(Some(&[der]), "peer-1").is_err());
    }

    #[tokio::test]
    async fn handshake_binds_both_identities_and_tunnel() {
        let (mut client, mut server) = duplex(4096);
        let server_task = tokio::spawn(async move {
            server_handshake(&mut server, "abroad", "iran", "tunnel-a").await
        });
        client_handshake(&mut client, "iran", "abroad", "tunnel-a")
            .await
            .unwrap();
        server_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn handshake_rejects_wrong_peer_identity() {
        let (mut client, mut server) = duplex(4096);
        let server_task = tokio::spawn(async move {
            server_handshake(&mut server, "abroad", "expected-peer", "tunnel-a").await
        });
        assert!(
            client_handshake(&mut client, "iran", "abroad", "tunnel-a")
                .await
                .is_err()
        );
        assert!(server_task.await.unwrap().is_err());
    }
}
