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

use crate::handshake;

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
