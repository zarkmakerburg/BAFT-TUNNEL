//! Stream engines (ADR-0018): a pluggable way to turn a TCP socket into a mutually
//! authenticated, encrypted byte stream bound to one enrolled peer identity.
//!
//! An engine only establishes and verifies the secure channel. Protocol hello, tunnel and
//! direction authorization, Yamux and stream limits stay in `tunnel.rs`, so every engine
//! is subject to the same fail-closed authorization. The engine id is `tcp-tls` for the
//! built-in TLS 1.3 mutual-authentication engine.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use futures::future::BoxFuture;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::TlsConnector;

use crate::{handshake, noise, session_status::Stage};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Dead-path detection for the long-lived tunnel TCP connection. A middlebox can silently
/// blackhole one flow while new flows still work; without these bounds the session is only
/// declared dead after Yamux's fixed 30 s ping timeout (30-45 s of stalled traffic in LAB runs
/// 43685882260 and 43706415887). The kernel aborts the socket once sent data (Yamux pings every
/// `tunnel::KEEPALIVE_INTERVAL`) stays unacknowledged this long, so the supervisor reconnects.
pub(crate) const TCP_USER_TIMEOUT: Duration = Duration::from_secs(20);
const TCP_KEEPIDLE: Duration = Duration::from_secs(10);
const TCP_KEEPINTVL: Duration = Duration::from_secs(5);
const TCP_KEEPCNT: u32 = 3;

/// Bound how long a silently dead tunnel connection can stay open (both dial and accept side).
/// Unprivileged per-socket options only; a failure is reported and the connection still works
/// with the default kernel timeouts.
pub(crate) fn tune_tunnel_socket(socket: &TcpStream) {
    if let Err(error) = set_dead_peer_bounds(socket) {
        eprintln!("tunnel socket: could not set dead-peer timeouts: {error}");
    }
    let _ = socket.set_nodelay(true);
}

#[cfg(target_os = "linux")]
fn set_dead_peer_bounds(socket: &TcpStream) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let fd = socket.as_raw_fd();
    let set = |level: libc::c_int, name: libc::c_int, value: libc::c_int| -> std::io::Result<()> {
        // SAFETY: `fd` is a live socket owned by `socket` for the duration of this call and
        // `value` is a plain c_int passed by pointer with its exact size.
        let rc = unsafe {
            libc::setsockopt(
                fd,
                level,
                name,
                (&value as *const libc::c_int).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    };
    set(libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1)?;
    set(
        libc::IPPROTO_TCP,
        libc::TCP_KEEPIDLE,
        TCP_KEEPIDLE.as_secs() as libc::c_int,
    )?;
    set(
        libc::IPPROTO_TCP,
        libc::TCP_KEEPINTVL,
        TCP_KEEPINTVL.as_secs() as libc::c_int,
    )?;
    set(
        libc::IPPROTO_TCP,
        libc::TCP_KEEPCNT,
        TCP_KEEPCNT as libc::c_int,
    )?;
    set(
        libc::IPPROTO_TCP,
        libc::TCP_USER_TIMEOUT,
        TCP_USER_TIMEOUT.as_millis() as libc::c_int,
    )
}

#[cfg(not(target_os = "linux"))]
fn set_dead_peer_bounds(_socket: &TcpStream) -> std::io::Result<()> {
    Ok(())
}

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
    acceptor: Option<Arc<crate::tls::ReloadingAcceptor>>,
    connector: Option<(TlsConnector, rustls::pki_types::ServerName<'static>)>,
    peer_id: String,
    peer_pins: String,
}

impl TlsEngine {
    pub fn listener(
        acceptor: Arc<crate::tls::ReloadingAcceptor>,
        peer_id: String,
        peer_pins: String,
    ) -> Self {
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
            tune_tunnel_socket(&socket);
            let acceptor = self
                .acceptor
                .as_ref()
                .context("engine is not configured as a listener")?
                .current()
                .context(Stage::SecureChannel)?;
            let secure = timeout(HANDSHAKE_TIMEOUT, acceptor.accept(socket))
                .await
                .context("TLS handshake timeout")
                .context(Stage::SecureChannel)?
                .context(Stage::SecureChannel)?;
            let certs = secure.get_ref().1.peer_certificates();
            handshake::verify_peer_fingerprint(certs, &self.peer_pins).context(Stage::Identity)?;
            handshake::verify_peer_identity(certs, &self.peer_id).context(Stage::Identity)?;
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
                .context("remote connect timeout")
                .context(Stage::Tcp)?
                .context(Stage::Tcp)?;
            tune_tunnel_socket(&socket);
            let secure = timeout(
                HANDSHAKE_TIMEOUT,
                connector.connect(server_name.clone(), socket),
            )
            .await
            .context("TLS handshake timeout")
            .context(Stage::SecureChannel)?
            .context(Stage::SecureChannel)?;
            let certs = secure.get_ref().1.peer_certificates();
            handshake::verify_peer_fingerprint(certs, &self.peer_pins).context(Stage::Identity)?;
            handshake::verify_peer_identity(certs, &self.peer_id).context(Stage::Identity)?;
            Ok(Box::new(secure) as BoxedSecureStream)
        })
    }
}

/// Noise IK over TCP (`noise-tcp`): X25519 / ChaCha20-Poly1305 / BLAKE2s with a pinned
/// peer static key. No certificates or CA; trust is the enrolled peer static public key.
/// The prologue binds the protocol version, both node identities, the tunnel and the
/// direction, so a key enrolled for one tunnel cannot be replayed into another.
pub struct NoiseEngine {
    local_private: Vec<u8>,
    peer_public: Vec<u8>,
    prologue: Vec<u8>,
}

/// Where the Noise static keys come from. The private key file holds 64 hex characters
/// (written by `noise-keygen`); the peer public key is 64 hex characters in the config.
#[derive(Debug, Clone)]
pub struct NoiseSettings {
    pub private_key: std::path::PathBuf,
    pub peer_public: String,
}

/// Generate a static keypair, write the private key as hex (create-new, 0600) and return
/// the public key as hex.
pub fn write_noise_keypair(path: &std::path::Path) -> Result<String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let pair = noise::generate_static_keypair()?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create {} (it must not already exist)", path.display()))?;
    file.write_all(hex::encode(&pair.private).as_bytes())?;
    file.write_all(b"\n")?;
    Ok(hex::encode(&pair.public))
}

impl NoiseSettings {
    /// Check key material without touching the network.
    pub fn validate(&self) -> Result<()> {
        let text = std::fs::read_to_string(&self.private_key).context("read Noise private key")?;
        decode_key(&text, "Noise private key")?;
        decode_key(&self.peer_public, "noise_peer_public")?;
        Ok(())
    }
}

fn decode_key(hex_text: &str, what: &str) -> Result<Vec<u8>> {
    let bytes = hex::decode(hex_text.trim()).with_context(|| format!("{what} must be hex"))?;
    if bytes.len() != 32 {
        anyhow::bail!("{what} must be exactly 32 bytes (64 hex characters)");
    }
    Ok(bytes)
}

impl NoiseEngine {
    /// Build from config. The initiator is the dialing node, the responder the listening one.
    pub fn from_settings(
        settings: &NoiseSettings,
        initiator_id: &str,
        responder_id: &str,
        tunnel_id: &str,
        direction: crate::authorization::ConnectionDirection,
    ) -> Result<Self> {
        let private = std::fs::read_to_string(&settings.private_key)
            .context("read Noise private key")
            .and_then(|text| decode_key(&text, "Noise private key"))?;
        let peer_public = decode_key(&settings.peer_public, "noise_peer_public")?;
        Ok(Self::new(
            private,
            peer_public,
            initiator_id,
            responder_id,
            tunnel_id,
            direction,
        ))
    }

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
            tune_tunnel_socket(&socket);
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
            .context("Noise handshake timeout")
            .context(Stage::SecureChannel)?
            .context(Stage::SecureChannel)?;
            Ok(Box::new(secure) as BoxedSecureStream)
        })
    }

    fn dial(&self, remote: SocketAddr) -> BoxFuture<'_, Result<BoxedSecureStream>> {
        Box::pin(async move {
            let socket = timeout(CONNECT_TIMEOUT, TcpStream::connect(remote))
                .await
                .context("remote connect timeout")
                .context(Stage::Tcp)?
                .context(Stage::Tcp)?;
            tune_tunnel_socket(&socket);
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
            .context("Noise handshake timeout")
            .context(Stage::SecureChannel)?
            .context(Stage::SecureChannel)?;
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

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn tunnel_socket_gets_dead_peer_bounds() {
        use std::os::fd::AsRawFd;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let socket = TcpStream::connect(addr).await.unwrap();
        tune_tunnel_socket(&socket);
        let get = |level: libc::c_int, name: libc::c_int| -> libc::c_int {
            let mut value: libc::c_int = 0;
            let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
            // SAFETY: valid fd, value/len point to a correctly sized c_int.
            let rc = unsafe {
                libc::getsockopt(
                    socket.as_raw_fd(),
                    level,
                    name,
                    (&mut value as *mut libc::c_int).cast(),
                    &mut len,
                )
            };
            assert_eq!(rc, 0);
            value
        };
        assert_eq!(get(libc::SOL_SOCKET, libc::SO_KEEPALIVE), 1);
        assert_eq!(get(libc::IPPROTO_TCP, libc::TCP_KEEPIDLE), 10);
        assert_eq!(get(libc::IPPROTO_TCP, libc::TCP_KEEPINTVL), 5);
        assert_eq!(get(libc::IPPROTO_TCP, libc::TCP_KEEPCNT), 3);
        assert_eq!(get(libc::IPPROTO_TCP, libc::TCP_USER_TIMEOUT), 20_000);
        assert_eq!(get(libc::IPPROTO_TCP, libc::TCP_NODELAY), 1);
    }

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
