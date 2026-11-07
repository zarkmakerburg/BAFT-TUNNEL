//! Stream engines (ADR-0018): a pluggable way to turn a TCP socket into a mutually
//! authenticated, encrypted byte stream bound to one enrolled peer identity.
//!
//! An engine only establishes and verifies the secure channel. Protocol hello, tunnel and
//! direction authorization, Yamux and stream limits stay in `tunnel.rs`, so every engine
//! is subject to the same fail-closed authorization. The engine id is `tcp-tls` for the
//! built-in TLS 1.3 mutual-authentication engine.

use std::{net::SocketAddr, time::Duration};

use anyhow::{Context, Result};
use futures::future::BoxFuture;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::{handshake, noise};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

pub trait SecureStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> SecureStream for T {}
pub type BoxedSecureStream = Box<dyn SecureStream>;

pub trait StreamEngine: Send + Sync + 'static {
    /// Stable identifier used in configuration and diagnostics.
    fn id(&self) -> &'static str;
    /// Secure and verify an inbound socket (listener side). Must fail closed.
    fn accept(&self, socket: TcpStream) -> BoxFuture<'_, Result<BoxedSecureStream>>;
    /// Connect, secure and verify the outbound peer (dialer side). Must fail closed.
    fn dial(&self, remote: SocketAddr) -> BoxFuture<'_, Result<BoxedSecureStream>>;
}

/// TLS 1.3 + mutual certificate authentication, SHA-256 pin(s) and URI SAN identity.
pub struct TlsEngine {
    acceptor: Option<TlsAcceptor>,
    connector: Option<(TlsConnector, rustls::pki_types::ServerName<'static>)>,
    peer_id: String,
    peer_pins: String,
}

impl TlsEngine {
    pub fn listener(acceptor: TlsAcceptor, peer_id: String, peer_pins: String) -> Self {
        Self {
            acceptor: Some(acceptor),
            connector: None,
            peer_id,
            peer_pins,
        }
    }

    pub fn dialer(
        connector: TlsConnector,
        server_name: rustls::pki_types::ServerName<'static>,
        peer_id: String,
        peer_pins: String,
    ) -> Self {
        Self {
            acceptor: None,
            connector: Some((connector, server_name)),
            peer_id,
            peer_pins,
        }
    }
}

impl StreamEngine for TlsEngine {
    fn id(&self) -> &'static str {
        "tcp-tls"
    }

    fn accept(&self, socket: TcpStream) -> BoxFuture<'_, Result<BoxedSecureStream>> {
        Box::pin(async move {
            let acceptor = self
                .acceptor
                .as_ref()
                .context("engine is not configured as a listener")?;
            let secure = timeout(HANDSHAKE_TIMEOUT, acceptor.accept(socket))
                .await
                .context("TLS handshake timeout")??;
            let certs = secure.get_ref().1.peer_certificates();
            handshake::verify_peer_fingerprint(certs, &self.peer_pins)?;
            handshake::verify_peer_identity(certs, &self.peer_id)?;
            Ok(Box::new(secure) as BoxedSecureStream)
        })
    }

    fn dial(&self, remote: SocketAddr) -> BoxFuture<'_, Result<BoxedSecureStream>> {
        Box::pin(async move {
            let (connector, server_name) = self
                .connector
                .as_ref()
                .context("engine is not configured as a dialer")?;
            let socket = timeout(CONNECT_TIMEOUT, TcpStream::connect(remote))
                .await
                .context("remote connect timeout")??;
            let secure = timeout(
                HANDSHAKE_TIMEOUT,
                connector.connect(server_name.clone(), socket),
            )
            .await
            .context("TLS handshake timeout")??;
            let certs = secure.get_ref().1.peer_certificates();
            handshake::verify_peer_fingerprint(certs, &self.peer_pins)?;
            handshake::verify_peer_identity(certs, &self.peer_id)?;
            Ok(Box::new(secure) as BoxedSecureStream)
        })
    }
}

/// Noise IK over TCP (`noise-tcp`): X25519 / ChaCha20-Poly1305 / BLAKE2s with a pinned
/// peer static key. No certificates or CA; trust is the enrolled peer static public key.
/// The prologue binds the protocol version, both node identities, the tunnel and the
/// direction, so a key enrolled for one tunnel cannot be replayed into another.
// Not selectable from configuration yet (P2-004c); covered by the engine tests below.
#[allow(dead_code)]
pub struct NoiseEngine {
    local_private: Vec<u8>,
    peer_public: Vec<u8>,
    prologue: Vec<u8>,
}

#[allow(dead_code)]
impl NoiseEngine {
    /// `initiator_id` is the dialing node and `responder_id` the listening node, on both
    /// sides, so the two prologues are byte-identical.
    pub fn new(
        local_private: Vec<u8>,
        peer_public: Vec<u8>,
        initiator_id: &str,
        responder_id: &str,
        tunnel_id: &str,
        direction: crate::authorization::ConnectionDirection,
    ) -> Self {
        let prologue = format!(
            "baft-tunnel/noise-tcp/v1\0{initiator_id}\0{responder_id}\0{tunnel_id}\0{}",
            direction.as_str()
        )
        .into_bytes();
        Self {
            local_private,
            peer_public,
            prologue,
        }
    }
}

impl StreamEngine for NoiseEngine {
    fn id(&self) -> &'static str {
        "noise-tcp"
    }

    fn accept(&self, socket: TcpStream) -> BoxFuture<'_, Result<BoxedSecureStream>> {
        Box::pin(async move {
            let secure = timeout(
                HANDSHAKE_TIMEOUT,
                noise::accept(
                    socket,
                    &self.local_private,
                    &self.peer_public,
                    &self.prologue,
                ),
            )
            .await
            .context("Noise handshake timeout")??;
            Ok(Box::new(secure) as BoxedSecureStream)
        })
    }

    fn dial(&self, remote: SocketAddr) -> BoxFuture<'_, Result<BoxedSecureStream>> {
        Box::pin(async move {
            let socket = timeout(CONNECT_TIMEOUT, TcpStream::connect(remote))
                .await
                .context("remote connect timeout")??;
            let secure = timeout(
                HANDSHAKE_TIMEOUT,
                noise::initiate(
                    socket,
                    &self.local_private,
                    &self.peer_public,
                    &self.prologue,
                ),
            )
            .await
            .context("Noise handshake timeout")??;
            Ok(Box::new(secure) as BoxedSecureStream)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authorization::ConnectionDirection;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn pair(
        tunnel_server: &str,
        direction_server: ConnectionDirection,
        wrong_client_key: bool,
    ) -> (NoiseEngine, NoiseEngine) {
        let server = noise::generate_static_keypair().unwrap();
        let client = noise::generate_static_keypair().unwrap();
        let impostor = noise::generate_static_keypair().unwrap();
        let listener = NoiseEngine::new(
            server.private.clone(),
            client.public.clone(),
            "iran",
            "abroad",
            tunnel_server,
            direction_server,
        );
        let dialer = NoiseEngine::new(
            if wrong_client_key {
                impostor.private
            } else {
                client.private
            },
            server.public,
            "iran",
            "abroad",
            "demo",
            ConnectionDirection::Direct,
        );
        (listener, dialer)
    }

    async fn exchange(listener: NoiseEngine, dialer: NoiseEngine) -> bool {
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = tcp.accept().await.unwrap();
            let Ok(mut stream) = listener.accept(socket).await else {
                return false;
            };
            let mut byte = [0_u8; 1];
            stream.read_exact(&mut byte).await.is_ok() && stream.write_all(&byte).await.is_ok()
        });
        let client = async {
            let mut stream = dialer.dial(addr).await.ok()?;
            stream.write_all(b"x").await.ok()?;
            let mut byte = [0_u8; 1];
            stream.read_exact(&mut byte).await.ok()?;
            Some(byte[0] == b'x')
        };
        let client_ok = timeout(Duration::from_secs(5), client)
            .await
            .ok()
            .flatten()
            .unwrap_or(false);
        let server_ok = timeout(Duration::from_secs(5), server)
            .await
            .map(|joined| joined.unwrap_or(false))
            .unwrap_or(false);
        client_ok && server_ok
    }

    #[tokio::test]
    async fn noise_engine_authenticates_enrolled_peer_and_relays_bytes() {
        let (listener, dialer) = pair("demo", ConnectionDirection::Direct, false);
        assert_eq!(listener.id(), "noise-tcp");
        assert!(exchange(listener, dialer).await);
    }

    #[tokio::test]
    async fn noise_engine_rejects_unenrolled_key_and_mismatched_tunnel_or_direction() {
        let (listener, dialer) = pair("demo", ConnectionDirection::Direct, true);
        assert!(!exchange(listener, dialer).await);
        let (listener, dialer) = pair("other-tunnel", ConnectionDirection::Direct, false);
        assert!(!exchange(listener, dialer).await);
        let (listener, dialer) = pair("demo", ConnectionDirection::Reverse, false);
        assert!(!exchange(listener, dialer).await);
    }
}
