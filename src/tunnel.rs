use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use crate::{handshake, tls};
use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time::timeout,
};
use tokio_rustls::client::TlsStream as ClientTlsStream;
use tokio_yamux::{Config as YamuxConfig, Session};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_SESSIONS: usize = 32;

pub struct ServerOptions {
    pub listen: String,
    pub target: String,
    pub local_id: String,
    pub peer_id: String,
    pub tunnel_id: String,
    pub peer_fingerprint: String,
    pub ca: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
    pub max_streams: usize,
}

pub struct ForwardOptions {
    pub listen: String,
    pub remote: String,
    pub server_name: String,
    pub local_id: String,
    pub peer_id: String,
    pub tunnel_id: String,
    pub peer_fingerprint: String,
    pub ca: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
    pub max_streams: usize,
}

fn yamux_config(max: usize) -> YamuxConfig {
    YamuxConfig {
        accept_backlog: max,
        enable_keepalive: true,
        keepalive_interval: Duration::from_secs(15),
        connection_write_timeout: Duration::from_secs(10),
        max_stream_count: max,
        max_stream_window_size: 1024 * 1024,
    }
}

fn socket(value: &str, name: &str) -> Result<SocketAddr> {
    value
        .parse()
        .with_context(|| format!("{name} must be an IP:port socket address"))
}

pub async fn serve(options: ServerOptions) -> Result<()> {
    let listen = socket(&options.listen, "--listen")?;
    let acceptor = tls::server_config(&options.ca, &options.cert, &options.key)?;
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("bind {listen}"))?;
    let sessions = Arc::new(Semaphore::new(MAX_SESSIONS));
    let streams = Arc::new(Semaphore::new(options.max_streams));
    eprintln!("BAFT-TUNNEL server listening on {listen}");

    loop {
        let (socket, remote) = listener.accept().await?;
        let permit = match sessions.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                eprintln!("connection rejected: session capacity reached");
                drop(socket);
                continue;
            }
        };
        let acceptor = acceptor.clone();
        let (local, peer, tunnel, pin, target, max, stream_slots) = (
            options.local_id.clone(),
            options.peer_id.clone(),
            options.tunnel_id.clone(),
            options.peer_fingerprint.clone(),
            options.target.clone(),
            options.max_streams,
            streams.clone(),
        );
        tokio::spawn(async move {
            let _permit = permit;
            let result: Result<()> = async {
                let mut secure = timeout(HANDSHAKE_TIMEOUT, acceptor.accept(socket))
                    .await
                    .context("TLS handshake timeout")??;
                handshake::verify_peer_fingerprint(secure.get_ref().1.peer_certificates(), &pin)?;
                timeout(
                    HANDSHAKE_TIMEOUT,
                    handshake::server_handshake(&mut secure, &local, &peer, &tunnel),
                )
                .await
                .context("protocol handshake timeout")??;
                server_session(secure, target, max, stream_slots).await;
                Ok(())
            }
            .await;
            if let Err(error) = result {
                eprintln!("peer session from {remote} closed: {error:#}");
            }
        });
    }
}

async fn server_session<S>(stream: S, target: String, max: usize, slots: Arc<Semaphore>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut session = Session::new_server(stream, yamux_config(max));
    while let Some(item) = session.next().await {
        match item {
            Ok(mut logical) => {
                let permit = match slots.clone().try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        drop(logical);
                        continue;
                    }
                };
                let destination = target.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let result: Result<()> = async {
                        let mut upstream =
                            timeout(CONNECT_TIMEOUT, TcpStream::connect(&destination))
                                .await
                                .context("destination connect timeout")?
                                .with_context(|| {
                                    format!("connect configured destination {destination}")
                                })?;
                        copy_bidirectional(&mut logical, &mut upstream)
                            .await
                            .context("forward TCP stream")?;
                        Ok(())
                    }
                    .await;
                    if let Err(error) = result {
                        eprintln!("forwarded stream closed: {error:#}");
                    }
                });
            }
            Err(error) => {
                eprintln!("yamux session failed: {error}");
                break;
            }
        }
    }
}

pub async fn forward(options: ForwardOptions) -> Result<()> {
    let listen = socket(&options.listen, "--listen")?;
    let remote = socket(&options.remote, "--remote")?;
    let connector = tls::connector(tls::client_config(
        &options.ca,
        &options.cert,
        &options.key,
    )?);
    let server_name = tls::server_name(&options.server_name)?;
    let socket = timeout(CONNECT_TIMEOUT, TcpStream::connect(remote))
        .await
        .context("remote connect timeout")??;
    let mut secure: ClientTlsStream<TcpStream> =
        timeout(HANDSHAKE_TIMEOUT, connector.connect(server_name, socket))
            .await
            .context("TLS handshake timeout")??;
    handshake::verify_peer_fingerprint(
        secure.get_ref().1.peer_certificates(),
        &options.peer_fingerprint,
    )?;
    timeout(
        HANDSHAKE_TIMEOUT,
        handshake::client_handshake(
            &mut secure,
            &options.local_id,
            &options.peer_id,
            &options.tunnel_id,
        ),
    )
    .await
    .context("protocol handshake timeout")??;

    let mut session = Session::new_client(secure, yamux_config(options.max_streams));
    let control = session.control();
    tokio::spawn(async move {
        while let Some(item) = session.next().await {
            match item {
                Ok(unexpected) => drop(unexpected),
                Err(error) => {
                    eprintln!("yamux session ended: {error}");
                    break;
                }
            }
        }
    });

    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("bind {listen}"))?;
    let slots = Arc::new(Semaphore::new(options.max_streams));
    eprintln!("BAFT-TUNNEL local forwarder listening on {listen} via {remote}");
    loop {
        let (mut local, _) = listener.accept().await?;
        let permit = match slots.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                drop(local);
                continue;
            }
        };
        let mut control = control.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let result: Result<()> = async {
                let mut stream = control
                    .open_stream()
                    .await
                    .context("open multiplexed stream")?;
                copy_bidirectional(&mut local, &mut stream)
                    .await
                    .context("forward local TCP stream")?;
                Ok(())
            }
            .await;
            if let Err(error) = result {
                eprintln!("local connection closed: {error:#}");
            }
        });
    }
}
