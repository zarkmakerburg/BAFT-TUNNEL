use crate::authorization::{
    ConnectionDirection, PHASE1_CAPABILITIES, StreamCapability, authorize_session, authorize_stream,
};
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
    direction: ConnectionDirection,
    capabilities: Vec<StreamCapability>,
    nonce: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerHello {
    version: u16,
    server_id: String,
    client_id: String,
    tunnel_id: String,
    direction: ConnectionDirection,
    capabilities: Vec<StreamCapability>,
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
    direction: ConnectionDirection,
) -> Result<()> {
    let client_nonce = nonce();
    write_message(
        stream,
        &ClientHello {
            version: VERSION,
            client_id: local.into(),
            server_id: peer.into(),
            tunnel_id: tunnel.into(),
            direction,
            capabilities: PHASE1_CAPABILITIES.to_vec(),
            nonce: client_nonce.clone(),
        },
    )
    .await?;
    let response: ServerHello = read_message(stream).await?;
    if response.version != VERSION
        || response.server_id != peer
        || response.client_id != local
        || response.tunnel_id != tunnel
        || response.direction != direction
        || response.capabilities != PHASE1_CAPABILITIES
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
    direction: ConnectionDirection,
) -> Result<()> {
    let request: ClientHello = read_message(stream).await?;
    if request.version != VERSION
        || request.client_id != peer
        || request.server_id != local
        || !valid_nonce(&request.nonce)
    {
        bail!("peer rejected or returned an invalid protocol handshake");
    }
    authorize_session(
        &request.client_id,
        peer,
        &request.tunnel_id,
        tunnel,
        request.direction,
        direction,
        &request.capabilities,
    )?;
    write_message(
        stream,
        &ServerHello {
            version: VERSION,
            server_id: local.into(),
            client_id: peer.into(),
            tunnel_id: tunnel.into(),
            direction,
            capabilities: PHASE1_CAPABILITIES.to_vec(),
            client_nonce: request.nonce,
            server_nonce: nonce(),
        },
    )
    .await
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamOpen {
    version: u16,
    tunnel_id: String,
    direction: ConnectionDirection,
    capability: StreamCapability,
}

pub async fn client_stream_open<S: AsyncWrite + Unpin>(
    stream: &mut S,
    tunnel: &str,
    direction: ConnectionDirection,
    capability: StreamCapability,
) -> Result<()> {
    authorize_stream(tunnel, tunnel, direction, direction, capability)?;
    write_message(
        stream,
        &StreamOpen {
            version: VERSION,
            tunnel_id: tunnel.into(),
            direction,
            capability,
        },
    )
    .await
}

pub async fn server_stream_open<S: AsyncRead + Unpin>(
    stream: &mut S,
    expected_tunnel: &str,
    expected_direction: ConnectionDirection,
) -> Result<()> {
    let request: StreamOpen = read_message(stream).await?;
    if request.version != VERSION {
        bail!("unsupported stream-open protocol version");
    }
    authorize_stream(
        &request.tunnel_id,
        expected_tunnel,
        request.direction,
        expected_direction,
        request.capability,
    )
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
    let (remaining, certificate) = x509_parser::parse_x509_certificate(leaf.as_ref())
        .map_err(|_| anyhow::anyhow!("TLS peer certificate is not valid X.509 DER"))?;
    if !remaining.is_empty() {
        bail!("TLS peer certificate contains trailing data after X.509 DER");
    }
    let san = certificate
        .subject_alternative_name()
        .context("parse TLS peer certificate subject alternative name")?
        .context("TLS peer certificate has no subject alternative name")?;
    let expected_uri = format!("urn:baft-tunnel:node:{expected_peer_id}");
    let mut node_identity = None;
    for name in &san.value.general_names {
        if let x509_parser::extensions::GeneralName::URI(uri) = name {
            if uri.starts_with("urn:baft-tunnel:node:") && node_identity.replace(*uri).is_some() {
                bail!("TLS certificate contains multiple BAFT-TUNNEL node identities");
            }
        }
    }
    if node_identity != Some(expected_uri.as_str()) {
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
    fn certificate_with_multiple_node_uri_sans_is_rejected() {
        use rcgen::{CertificateParams, KeyPair, SanType, string::Ia5String};

        let mut params = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
        params.subject_alt_names.extend([
            SanType::URI(Ia5String::try_from("urn:baft-tunnel:node:peer-1").unwrap()),
            SanType::URI(Ia5String::try_from("urn:baft-tunnel:node:peer-2").unwrap()),
        ]);
        let key = KeyPair::generate().unwrap();
        let certificate = params.self_signed(&key).unwrap();
        let der = rustls::pki_types::CertificateDer::from(certificate.der().to_vec());
        assert!(verify_peer_identity(Some(&[der.clone()]), "peer-1").is_err());
        assert!(verify_peer_identity(Some(&[der]), "peer-2").is_err());
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
            server_handshake(
                &mut server,
                "abroad",
                "iran",
                "tunnel-a",
                ConnectionDirection::Direct,
            )
            .await
        });
        client_handshake(
            &mut client,
            "iran",
            "abroad",
            "tunnel-a",
            ConnectionDirection::Direct,
        )
        .await
        .unwrap();
        server_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn handshake_rejects_wrong_connection_direction() {
        let (mut client, mut server) = duplex(4096);
        let server_task = tokio::spawn(async move {
            server_handshake(
                &mut server,
                "abroad",
                "iran",
                "tunnel-a",
                ConnectionDirection::Reverse,
            )
            .await
        });
        assert!(
            client_handshake(
                &mut client,
                "iran",
                "abroad",
                "tunnel-a",
                ConnectionDirection::Direct
            )
            .await
            .is_err()
        );
        assert!(server_task.await.unwrap().is_err());
    }

    #[test]
    fn stream_open_rejects_unknown_capabilities() {
        let request =
            br#"{"version":1,"tunnel_id":"lab","direction":"direct","capability":"udp-forward"}"#;
        assert!(serde_json::from_slice::<StreamOpen>(request).is_err());
    }

    #[tokio::test]
    async fn stream_open_rejects_wrong_tunnel_or_direction() {
        let (mut client, mut server) = duplex(4096);
        let client_task = tokio::spawn(async move {
            client_stream_open(
                &mut client,
                "tunnel-a",
                ConnectionDirection::Direct,
                StreamCapability::TcpForward,
            )
            .await
        });
        assert!(
            server_stream_open(&mut server, "tunnel-a", ConnectionDirection::Reverse)
                .await
                .is_err()
        );
        client_task.await.unwrap().unwrap();

        let (mut client, mut server) = duplex(4096);
        let client_task = tokio::spawn(async move {
            client_stream_open(
                &mut client,
                "tunnel-a",
                ConnectionDirection::Direct,
                StreamCapability::TcpForward,
            )
            .await
        });
        assert!(
            server_stream_open(&mut server, "tunnel-b", ConnectionDirection::Direct)
                .await
                .is_err()
        );
        client_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn handshake_rejects_wrong_peer_identity() {
        let (mut client, mut server) = duplex(4096);
        let server_task = tokio::spawn(async move {
            server_handshake(
                &mut server,
                "abroad",
                "expected-peer",
                "tunnel-a",
                ConnectionDirection::Direct,
            )
            .await
        });
        assert!(
            client_handshake(
                &mut client,
                "iran",
                "abroad",
                "tunnel-a",
                ConnectionDirection::Direct
            )
            .await
            .is_err()
        );
        assert!(server_task.await.unwrap().is_err());
    }
}
