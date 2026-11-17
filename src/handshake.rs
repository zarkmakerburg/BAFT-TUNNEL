use crate::authorization::PROTOCOL_V1;

use crate::authorization::{
    ConnectionDirection, StreamCapability, authorize_session_versioned, authorize_stream_versioned,
    capabilities_for_version,
};
use anyhow::{Context, Result, bail};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[allow(dead_code)]
const VERSION: u16 = PROTOCOL_V1;
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
    /// Protocol v3 only: single-use resume token (16 random bytes, hex). Absent for v1/v2 and
    /// rejected there, so an older peer can never be handed one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resume_token: Option<String>,
}

const RESUME_TOKEN_BYTES: usize = 16;

/// Material both sides hold after a v3 handshake. The token is the bearer secret; the binding is
/// `resume_binding` over the original handshake, kept so a later resume can prove it belongs to
/// the same identities, tunnel and direction (ADR-0040).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResumeGrant {
    pub token: [u8; RESUME_TOKEN_BYTES],
    pub binding: [u8; 32],
}

/// `SHA-256("baft-resume-v1" || len-prefixed client_id, server_id, tunnel_id, direction, client_nonce,
/// server_nonce)`. Every field is length-prefixed so two different field splits can never hash
/// the same.
pub fn resume_binding(
    client_id: &str,
    server_id: &str,
    tunnel_id: &str,
    direction: ConnectionDirection,
    client_nonce: &str,
    server_nonce: &str,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"baft-resume-v1");
    for field in [
        client_id,
        server_id,
        tunnel_id,
        direction.as_str(),
        client_nonce,
        server_nonce,
    ] {
        h.update((field.len() as u32).to_be_bytes());
        h.update(field.as_bytes());
    }
    h.finalize().into()
}

fn decode_token(value: &str) -> Option<[u8; RESUME_TOKEN_BYTES]> {
    if value.len() != RESUME_TOKEN_BYTES * 2 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0_u8; RESUME_TOKEN_BYTES];
    hex::decode_to_slice(value, &mut out).ok()?;
    Some(out)
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

/// Maximum number of simultaneously accepted peer pins (ADR-0017 B1 rollover window).
pub const MAX_PEER_PINS: usize = 2;

pub(crate) fn normalize_pin(pin: &str) -> String {
    pin.replace(':', "").to_ascii_lowercase()
}

/// A peer pin value is one SHA-256 hex digest, or two comma-separated digests during
/// a certificate rollover. More than two, empty entries and duplicates are rejected.
pub fn validate_fingerprint(value: &str) -> Result<()> {
    let pins: Vec<&str> = value.split(',').map(str::trim).collect();
    if pins.len() > MAX_PEER_PINS {
        bail!("at most {MAX_PEER_PINS} peer fingerprints are allowed (rollover window)");
    }
    let mut seen: Vec<String> = Vec::new();
    for pin in pins {
        let normalized = normalize_pin(pin);
        if normalized.len() != 64 || !normalized.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("peer fingerprint must be a SHA-256 hex digest (64 hex characters)");
        }
        if seen.contains(&normalized) {
            bail!("duplicate peer fingerprint");
        }
        seen.push(normalized);
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

#[allow(dead_code)]
pub async fn client_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    local: &str,
    peer: &str,
    tunnel: &str,
    direction: ConnectionDirection,
) -> Result<()> {
    client_handshake_versioned(stream, local, peer, tunnel, direction, PROTOCOL_V1).await
}

pub async fn client_handshake_versioned<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    local: &str,
    peer: &str,
    tunnel: &str,
    direction: ConnectionDirection,
    version: u16,
) -> Result<()> {
    client_handshake_inner(stream, local, peer, tunnel, direction, version)
        .await
        .map(|_| ())
}

/// v3 client handshake that also returns the resume grant. Refuses any other version.
#[allow(dead_code)]
pub async fn client_handshake_grant<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    local: &str,
    peer: &str,
    tunnel: &str,
    direction: ConnectionDirection,
) -> Result<ResumeGrant> {
    client_handshake_inner(
        stream,
        local,
        peer,
        tunnel,
        direction,
        crate::authorization::PROTOCOL_V3,
    )
    .await?
    .context("v3 handshake returned no resume grant")
}

async fn client_handshake_inner<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    local: &str,
    peer: &str,
    tunnel: &str,
    direction: ConnectionDirection,
    version: u16,
) -> Result<Option<ResumeGrant>> {
    let capabilities = capabilities_for_version(version)?;
    let client_nonce = nonce();
    write_message(
        stream,
        &ClientHello {
            version,
            client_id: local.into(),
            server_id: peer.into(),
            tunnel_id: tunnel.into(),
            direction,
            capabilities: capabilities.to_vec(),
            nonce: client_nonce.clone(),
        },
    )
    .await?;
    let response: ServerHello = read_message(stream).await?;
    if response.version != version {
        bail!(
            "protocol version mismatch: configured v{version}, peer returned v{}",
            response.version
        );
    }
    if response.server_id != peer
        || response.client_id != local
        || response.tunnel_id != tunnel
        || response.direction != direction
        || response.capabilities != capabilities
        || response.client_nonce != client_nonce
        || !valid_nonce(&response.server_nonce)
        || response.server_nonce == client_nonce
    {
        bail!("peer rejected or returned an invalid protocol handshake");
    }
    // v3 requires a well-formed token; v1/v2 must not carry one (no downgrade, no smuggling).
    if version == crate::authorization::PROTOCOL_V3 {
        let token = response
            .resume_token
            .as_deref()
            .and_then(decode_token)
            .context("v3 peer returned a missing or malformed resume token")?;
        let binding = resume_binding(
            local,
            peer,
            tunnel,
            direction,
            &client_nonce,
            &response.server_nonce,
        );
        return Ok(Some(ResumeGrant { token, binding }));
    }
    if response.resume_token.is_some() {
        bail!("peer returned a resume token for a protocol that has none");
    }
    Ok(None)
}

#[allow(dead_code)]
pub async fn server_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    local: &str,
    peer: &str,
    tunnel: &str,
    direction: ConnectionDirection,
) -> Result<()> {
    server_handshake_versioned(stream, local, peer, tunnel, direction, PROTOCOL_V1).await
}

pub async fn server_handshake_versioned<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    local: &str,
    peer: &str,
    tunnel: &str,
    direction: ConnectionDirection,
    version: u16,
) -> Result<()> {
    server_handshake_inner(stream, local, peer, tunnel, direction, version)
        .await
        .map(|_| ())
}

/// v3 server handshake that also returns the resume grant. Refuses any other version.
#[allow(dead_code)]
pub async fn server_handshake_grant<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    local: &str,
    peer: &str,
    tunnel: &str,
    direction: ConnectionDirection,
) -> Result<ResumeGrant> {
    server_handshake_inner(
        stream,
        local,
        peer,
        tunnel,
        direction,
        crate::authorization::PROTOCOL_V3,
    )
    .await?
    .context("v3 handshake returned no resume grant")
}

async fn server_handshake_inner<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    local: &str,
    peer: &str,
    tunnel: &str,
    direction: ConnectionDirection,
    version: u16,
) -> Result<Option<ResumeGrant>> {
    let capabilities = capabilities_for_version(version)?;
    let request: ClientHello = read_message(stream).await?;
    if request.version != version {
        bail!(
            "protocol version mismatch: configured v{version}, peer requested v{}",
            request.version
        );
    }
    if request.client_id != peer || request.server_id != local || !valid_nonce(&request.nonce) {
        bail!("peer rejected or returned an invalid protocol handshake");
    }
    authorize_session_versioned(
        version,
        &request.client_id,
        peer,
        &request.tunnel_id,
        tunnel,
        request.direction,
        direction,
        &request.capabilities,
    )?;
    if request.capabilities != capabilities {
        bail!("peer protocol capability set does not match configured version");
    }
    let server_nonce = nonce();
    let grant = if version == crate::authorization::PROTOCOL_V3 {
        let mut token = [0_u8; RESUME_TOKEN_BYTES];
        OsRng.fill_bytes(&mut token);
        Some(ResumeGrant {
            token,
            binding: resume_binding(
                peer,
                local,
                tunnel,
                direction,
                &request.nonce,
                &server_nonce,
            ),
        })
    } else {
        None
    };
    write_message(
        stream,
        &ServerHello {
            version,
            server_id: local.into(),
            client_id: peer.into(),
            tunnel_id: tunnel.into(),
            direction,
            capabilities: capabilities.to_vec(),
            client_nonce: request.nonce,
            server_nonce,
            resume_token: grant.as_ref().map(|g| hex::encode(g.token)),
        },
    )
    .await?;
    Ok(grant)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamOpen {
    version: u16,
    tunnel_id: String,
    direction: ConnectionDirection,
    capability: StreamCapability,
}

#[allow(dead_code)]
pub async fn client_stream_open<S: AsyncWrite + Unpin>(
    stream: &mut S,
    tunnel: &str,
    direction: ConnectionDirection,
    capability: StreamCapability,
) -> Result<()> {
    client_stream_open_versioned(stream, tunnel, direction, capability, PROTOCOL_V1).await
}

pub async fn client_stream_open_versioned<S: AsyncWrite + Unpin>(
    stream: &mut S,
    tunnel: &str,
    direction: ConnectionDirection,
    capability: StreamCapability,
    version: u16,
) -> Result<()> {
    authorize_stream_versioned(version, tunnel, tunnel, direction, direction, capability)?;
    write_message(
        stream,
        &StreamOpen {
            version,
            tunnel_id: tunnel.into(),
            direction,
            capability,
        },
    )
    .await
}

#[allow(dead_code)]
pub async fn server_stream_open<S: AsyncRead + Unpin>(
    stream: &mut S,
    expected_tunnel: &str,
    expected_direction: ConnectionDirection,
) -> Result<()> {
    server_stream_open_capability(stream, expected_tunnel, expected_direction, PROTOCOL_V1)
        .await
        .map(|_| ())
}

pub async fn server_stream_open_capability<S: AsyncRead + Unpin>(
    stream: &mut S,
    expected_tunnel: &str,
    expected_direction: ConnectionDirection,
    version: u16,
) -> Result<StreamCapability> {
    let request: StreamOpen = read_message(stream).await?;
    if request.version != version {
        bail!(
            "protocol version mismatch in stream-open: expected v{version}, got v{}",
            request.version
        );
    }
    authorize_stream_versioned(
        version,
        &request.tunnel_id,
        expected_tunnel,
        request.direction,
        expected_direction,
        request.capability,
    )?;
    Ok(request.capability)
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
    validate_fingerprint(expected)?;
    if !expected
        .split(',')
        .any(|pin| actual == normalize_pin(pin.trim()))
    {
        bail!("TLS peer certificate fingerprint does not match the configured pin");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authorization::PHASE1_CAPABILITIES;
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
        // Rollover window: exactly one or two distinct pins.
        let (a, b, c) = ("ab".repeat(32), "cd".repeat(32), "ef".repeat(32));
        assert!(validate_fingerprint(&format!("{a},{b}")).is_ok());
        assert!(validate_fingerprint(&format!("{a},{b},{c}")).is_err());
        assert!(validate_fingerprint(&format!("{a},{a}")).is_err());
        assert!(validate_fingerprint(&format!("{a},")).is_err());
        assert!(validate_fingerprint(&format!(",{a}")).is_err());
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
    async fn client_handshake_rejects_each_server_hello_binding_mismatch() {
        // A TLS-authenticated peer must still prove that its protocol response
        // belongs to this exact client hello, tunnel, direction and capability set.
        for mutation in 0..8 {
            let (mut client, mut server) = duplex(4096);
            let server_task = tokio::spawn(async move {
                let request: ClientHello = read_message(&mut server).await.unwrap();
                let mut response = ServerHello {
                    version: VERSION,
                    server_id: "abroad".into(),
                    client_id: "iran".into(),
                    tunnel_id: "tunnel-a".into(),
                    direction: ConnectionDirection::Direct,
                    capabilities: PHASE1_CAPABILITIES.to_vec(),
                    client_nonce: request.nonce.clone(),
                    server_nonce: nonce(),
                    resume_token: None,
                };
                match mutation {
                    0 => response.version = VERSION + 1,
                    1 => response.server_id = "unexpected-server".into(),
                    2 => response.client_id = "unexpected-client".into(),
                    3 => response.tunnel_id = "unexpected-tunnel".into(),
                    4 => response.direction = ConnectionDirection::Reverse,
                    5 => response.capabilities.clear(),
                    6 => response.client_nonce = nonce(),
                    7 => response.server_nonce = request.nonce,
                    _ => unreachable!(),
                }
                write_message(&mut server, &response).await.unwrap();
            });

            assert!(
                client_handshake(
                    &mut client,
                    "iran",
                    "abroad",
                    "tunnel-a",
                    ConnectionDirection::Direct,
                )
                .await
                .is_err(),
                "client accepted server hello mutation {mutation}"
            );
            server_task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn server_handshake_rejects_each_client_hello_authorization_mismatch() {
        for mutation in 0..8 {
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
            let mut request = ClientHello {
                version: VERSION,
                client_id: "iran".into(),
                server_id: "abroad".into(),
                tunnel_id: "tunnel-a".into(),
                direction: ConnectionDirection::Direct,
                capabilities: PHASE1_CAPABILITIES.to_vec(),
                nonce: nonce(),
            };
            match mutation {
                0 => request.version = VERSION + 1,
                1 => request.client_id = "unexpected-client".into(),
                2 => request.server_id = "unexpected-server".into(),
                3 => request.tunnel_id = "unexpected-tunnel".into(),
                4 => request.direction = ConnectionDirection::Reverse,
                5 => request.capabilities.clear(),
                6 => request.capabilities.push(StreamCapability::TcpForward),
                7 => request.nonce = "not-a-cryptographic-nonce".into(),
                _ => unreachable!(),
            }
            write_message(&mut client, &request).await.unwrap();
            assert!(
                server_task.await.unwrap().is_err(),
                "server accepted client hello mutation {mutation}"
            );
        }
    }

    #[tokio::test]
    async fn handshake_rejects_malformed_nonces_and_unknown_hello_fields() {
        // Short, non-hex, uppercase-padded and oversized nonces must never be accepted.
        for bad in ["", "ab", &"g".repeat(64), &"a".repeat(65), &"a".repeat(63)] {
            let (mut client, mut server) = duplex(4096);
            let bad_owned = bad.to_owned();
            let server_task = tokio::spawn(async move {
                let request: ClientHello = read_message(&mut server).await.unwrap();
                let response = ServerHello {
                    version: VERSION,
                    server_id: "abroad".into(),
                    client_id: "iran".into(),
                    tunnel_id: "tunnel-a".into(),
                    direction: ConnectionDirection::Direct,
                    capabilities: PHASE1_CAPABILITIES.to_vec(),
                    client_nonce: request.nonce,
                    server_nonce: bad_owned,
                    resume_token: None,
                };
                write_message(&mut server, &response).await.unwrap();
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
                .is_err(),
                "client accepted server nonce {bad:?}"
            );
            server_task.await.unwrap();
        }

        // A hello carrying an unknown field is rejected, not silently ignored.
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
        let mut value = serde_json::to_value(ClientHello {
            version: VERSION,
            client_id: "iran".into(),
            server_id: "abroad".into(),
            tunnel_id: "tunnel-a".into(),
            direction: ConnectionDirection::Direct,
            capabilities: PHASE1_CAPABILITIES.to_vec(),
            nonce: nonce(),
        })
        .unwrap();
        value["extra"] = serde_json::json!(true);
        write_message(&mut client, &value).await.unwrap();
        assert!(server_task.await.unwrap().is_err());
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

    #[tokio::test]
    async fn protocol_v2_handshake_succeeds_only_when_both_peers_are_v2() {
        let (mut client, mut server) = duplex(4096);
        let server_task = tokio::spawn(async move {
            server_handshake_versioned(
                &mut server,
                "abroad",
                "iran",
                "tunnel-a",
                ConnectionDirection::Direct,
                2,
            )
            .await
        });
        client_handshake_versioned(
            &mut client,
            "iran",
            "abroad",
            "tunnel-a",
            ConnectionDirection::Direct,
            2,
        )
        .await
        .unwrap();
        server_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn mixed_protocol_generations_are_rejected_without_retry_or_downgrade() {
        for (client_version, server_version) in [(1, 2), (2, 1)] {
            let (mut client, mut server) = duplex(4096);
            let server_task = tokio::spawn(async move {
                server_handshake_versioned(
                    &mut server,
                    "abroad",
                    "iran",
                    "tunnel-a",
                    ConnectionDirection::Direct,
                    server_version,
                )
                .await
            });
            let result = client_handshake_versioned(
                &mut client,
                "iran",
                "abroad",
                "tunnel-a",
                ConnectionDirection::Direct,
                client_version,
            )
            .await;
            assert!(
                result.is_err(),
                "mixed v{client_version}/v{server_version} accepted"
            );
            assert!(server_task.await.unwrap().is_err());
        }
    }

    #[tokio::test]
    async fn integrity_probe_stream_open_is_v2_only_and_separately_authorized() {
        let (mut client, mut server) = duplex(4096);
        let server_task = tokio::spawn(async move {
            server_stream_open_capability(&mut server, "tunnel-a", ConnectionDirection::Direct, 2)
                .await
        });
        client_stream_open_versioned(
            &mut client,
            "tunnel-a",
            ConnectionDirection::Direct,
            StreamCapability::IntegrityProbe,
            2,
        )
        .await
        .unwrap();
        assert_eq!(
            server_task.await.unwrap().unwrap(),
            StreamCapability::IntegrityProbe
        );

        let (mut client, _server) = duplex(4096);
        assert!(
            client_stream_open_versioned(
                &mut client,
                "tunnel-a",
                ConnectionDirection::Direct,
                StreamCapability::IntegrityProbe,
                1
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn stream_open_rejects_unknown_capabilities() {
        let request =
            br#"{"version":1,"tunnel_id":"lab","direction":"direct","capability":"udp-forward"}"#;
        assert!(serde_json::from_slice::<StreamOpen>(request).is_err());
    }

    #[test]
    fn stream_open_rejects_duplicate_and_unknown_fields() {
        let duplicate =
            br#"{"version":1,"version":1,"tunnel_id":"lab","direction":"direct","capability":"tcp-forward"}"#;
        let unknown =
            br#"{"version":1,"tunnel_id":"lab","direction":"direct","capability":"tcp-forward","extra":true}"#;
        assert!(serde_json::from_slice::<StreamOpen>(duplicate).is_err());
        assert!(serde_json::from_slice::<StreamOpen>(unknown).is_err());
    }

    #[tokio::test]
    async fn framed_reader_rejects_zero_and_oversized_lengths_before_allocation() {
        let (mut writer, mut reader) = duplex(16);
        writer.write_all(&0_u16.to_be_bytes()).await.unwrap();
        assert!(read_message::<_, StreamOpen>(&mut reader).await.is_err());

        let (mut writer, mut reader) = duplex(16);
        writer
            .write_all(&((MAX_MESSAGE as u16) + 1).to_be_bytes())
            .await
            .unwrap();
        assert!(read_message::<_, StreamOpen>(&mut reader).await.is_err());
    }

    #[tokio::test]
    async fn framed_reader_rejects_truncated_payload() {
        let (mut writer, mut reader) = duplex(16);
        writer.write_all(&5_u16.to_be_bytes()).await.unwrap();
        writer.write_all(b"{}").await.unwrap();
        writer.shutdown().await.unwrap();
        assert!(read_message::<_, StreamOpen>(&mut reader).await.is_err());
    }

    #[tokio::test]
    async fn framed_reader_survives_deterministic_adversarial_frame_corpus() {
        use std::time::Duration;

        // Deterministic, dependency-free mutation corpus. It exercises arbitrary
        // length prefixes and payload bytes while keeping each case bounded.
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        for _case in 0..1024 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let declared = (state as usize) % (MAX_MESSAGE + 5);
            let mut frame = Vec::with_capacity(2 + declared.min(MAX_MESSAGE));
            frame.extend_from_slice(&(declared as u16).to_be_bytes());
            if declared <= MAX_MESSAGE {
                for _ in 0..declared {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    frame.push(state as u8);
                }
            }

            let (mut writer, mut reader) = duplex(MAX_MESSAGE + 2);
            writer.write_all(&frame).await.unwrap();
            writer.shutdown().await.unwrap();
            let result = tokio::time::timeout(
                Duration::from_millis(50),
                read_message::<_, StreamOpen>(&mut reader),
            )
            .await;
            assert!(result.is_ok(), "frame parser stalled on corpus case");
        }
    }

    #[tokio::test]
    async fn stream_open_rejects_every_mutated_authorization_frame() {
        // Seeded field-level mutation of an otherwise valid stream-open frame:
        // only the exact (version, tunnel, direction, capability) tuple may pass.
        let valid =
            r#"{"version":1,"tunnel_id":"demo","direction":"direct","capability":"tcp-forward"}"#;
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        for _case in 0..512 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let mut bytes = valid.as_bytes().to_vec();
            let flips = 1 + (state as usize) % 3;
            for i in 0..flips {
                let pos = ((state >> (8 * i)) as usize) % bytes.len();
                bytes[pos] = (state >> (3 + i)) as u8;
            }
            let mut frame = (bytes.len() as u16).to_be_bytes().to_vec();
            frame.extend_from_slice(&bytes);
            let (mut writer, mut reader) = duplex(MAX_MESSAGE + 2);
            writer.write_all(&frame).await.unwrap();
            writer.shutdown().await.unwrap();
            let result = server_stream_open(&mut reader, "demo", ConnectionDirection::Direct).await;
            if result.is_ok() {
                assert_eq!(bytes, valid.as_bytes(), "mutated frame was authorized");
            }
        }
    }

    #[tokio::test]
    async fn framed_writer_rejects_oversized_serialized_message() {
        #[derive(serde::Serialize)]
        struct Oversized {
            payload: String,
        }

        let (mut writer, _reader) = duplex(16);
        let message = Oversized {
            payload: "x".repeat(MAX_MESSAGE + 1),
        };
        assert!(write_message(&mut writer, &message).await.is_err());
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

    /// v1, v2 and v3 peers reject each other in both directions: no silent downgrade.
    #[tokio::test]
    async fn protocol_v3_handshake_is_strict_against_v1_and_v2_in_both_directions() {
        use crate::authorization::{PROTOCOL_V2, PROTOCOL_V3};
        let run = |client_v: u16, server_v: u16| async move {
            let (mut c, mut s) = duplex(4096);
            let server = tokio::spawn(async move {
                server_handshake_versioned(
                    &mut s,
                    "abroad",
                    "iran",
                    "t",
                    ConnectionDirection::Direct,
                    server_v,
                )
                .await
            });
            let client = client_handshake_versioned(
                &mut c,
                "iran",
                "abroad",
                "t",
                ConnectionDirection::Direct,
                client_v,
            )
            .await;
            (client, server.await.unwrap())
        };
        let (c, s) = run(PROTOCOL_V3, PROTOCOL_V3).await;
        assert!(c.is_ok() && s.is_ok(), "v3 with v3 must work");
        for (cv, sv) in [
            (PROTOCOL_V3, PROTOCOL_V2),
            (PROTOCOL_V2, PROTOCOL_V3),
            (PROTOCOL_V3, 1),
            (1, PROTOCOL_V3),
        ] {
            let (c, s) = run(cv, sv).await;
            assert!(
                c.is_err() || s.is_err(),
                "v{cv} client vs v{sv} server must be rejected"
            );
            assert!(
                s.is_err(),
                "the server must refuse v{cv} when configured v{sv}"
            );
        }
        // Reverse is refused for v3 on the server side.
        let (mut c, mut s) = duplex(4096);
        let server = tokio::spawn(async move {
            server_handshake_versioned(
                &mut s,
                "abroad",
                "iran",
                "t",
                ConnectionDirection::Reverse,
                PROTOCOL_V3,
            )
            .await
        });
        let client = client_handshake_versioned(
            &mut c,
            "iran",
            "abroad",
            "t",
            ConnectionDirection::Reverse,
            PROTOCOL_V3,
        )
        .await;
        assert!(server.await.unwrap().is_err() && client.is_err());
    }

    /// A v3 client accepts only a well-formed token; v1/v2 clients refuse any token at all.
    #[tokio::test]
    async fn resume_token_is_required_for_v3_and_forbidden_otherwise() {
        use crate::authorization::{PROTOCOL_V2, PROTOCOL_V3, capabilities_for_version};
        for (version, token, ok) in [
            (PROTOCOL_V3, None, false),
            (PROTOCOL_V3, Some("zz"), false),
            (PROTOCOL_V3, Some("00112233445566778899aabbccddeeff"), true),
            (
                PROTOCOL_V3,
                Some("00112233445566778899aabbccddeeff00"),
                false,
            ),
            (PROTOCOL_V2, Some("00112233445566778899aabbccddeeff"), false),
            (PROTOCOL_V2, None, true),
            (1, Some("00112233445566778899aabbccddeeff"), false),
        ] {
            let (mut client, mut server) = duplex(4096);
            let caps = capabilities_for_version(version).unwrap().to_vec();
            let token = token.map(str::to_owned);
            let task = tokio::spawn(async move {
                let request: ClientHello = read_message(&mut server).await.unwrap();
                let response = ServerHello {
                    version,
                    server_id: "abroad".into(),
                    client_id: "iran".into(),
                    tunnel_id: "t".into(),
                    direction: ConnectionDirection::Direct,
                    capabilities: caps,
                    client_nonce: request.nonce,
                    server_nonce: nonce(),
                    resume_token: token,
                };
                write_message(&mut server, &response).await.unwrap();
            });
            let r = client_handshake_versioned(
                &mut client,
                "iran",
                "abroad",
                "t",
                ConnectionDirection::Direct,
                version,
            )
            .await;
            task.await.unwrap();
            assert_eq!(r.is_ok(), ok, "v{version} token handling");
        }
    }

    /// A v3 server never reuses a token across sessions and the grant functions refuse Reverse.
    #[tokio::test]
    async fn grant_functions_are_v3_direct_only() {
        let (mut c, mut s) = duplex(4096);
        let server = tokio::spawn(async move {
            server_handshake_grant(&mut s, "abroad", "iran", "t", ConnectionDirection::Reverse)
                .await
        });
        let client =
            client_handshake_grant(&mut c, "iran", "abroad", "t", ConnectionDirection::Reverse)
                .await;
        assert!(server.await.unwrap().is_err() && client.is_err());
    }
}
