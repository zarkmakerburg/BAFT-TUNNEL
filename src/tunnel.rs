use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use crate::{
    authorization::{ConnectionDirection, StreamCapability},
    engine::{BoxedSecureStream, NoiseEngine, NoiseSettings, StreamEngine, TlsEngine},
    handshake, tls,
};
use anyhow::{Context, Result, bail};
use futures::StreamExt;
use rand::{RngCore, rngs::OsRng};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{
        Semaphore,
        mpsc::{self, Receiver},
        watch,
    },
    task::JoinSet,
    time::{sleep, timeout},
};
use tokio_yamux::{Config as YamuxConfig, Session};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_SESSIONS: usize = 8;
/// Connections that have not finished authentication. Counted separately from
/// established sessions so unauthenticated sockets cannot evict a valid peer.
const MAX_PENDING_HANDSHAKES: usize = 32;
const MAX_PENDING_PER_IP: usize = 2;
/// A forwarded TCP stream with no data in a direction for this long is closed,
/// so stalled peers cannot hold stream slots forever.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(900);
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(200);
// Keep aggregate receive buffering bounded while allowing a 2 MiB payload
// to pass without depending on repeated window-update cycles over high-RTT links.
const MAX_STREAM_WINDOW_SIZE: u32 = 4 * 1024 * 1024;
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(20);
const STREAM_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("register SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

pub struct ServerOptions {
    pub listen: String,
    pub target: String,
    pub local_id: String,
    pub peer_id: String,
    pub tunnel_id: String,
    pub direction: ConnectionDirection,
    pub peer_fingerprint: String,
    pub ca: PathBuf,
    pub crl: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
    /// `Some` selects the `noise-tcp` engine; the TLS fields are then unused.
    pub noise: Option<NoiseSettings>,
    pub max_streams: usize,
}

pub struct ForwardOptions {
    pub listen: String,
    pub remote: String,
    pub server_name: String,
    pub local_id: String,
    pub peer_id: String,
    pub tunnel_id: String,
    pub direction: ConnectionDirection,
    pub peer_fingerprint: String,
    pub ca: PathBuf,
    pub crl: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
    /// `Some` selects the `noise-tcp` engine; the TLS fields are then unused.
    pub noise: Option<NoiseSettings>,
    pub max_streams: usize,
}

fn yamux_config(max: usize) -> YamuxConfig {
    YamuxConfig {
        accept_backlog: max,
        enable_keepalive: true,
        keepalive_interval: Duration::from_secs(15),
        connection_write_timeout: Duration::from_secs(10),
        max_stream_count: max,
        max_stream_window_size: MAX_STREAM_WINDOW_SIZE,
    }
}

fn socket(value: &str, name: &str) -> Result<SocketAddr> {
    value
        .parse()
        .with_context(|| format!("{name} must be an IP:port socket address"))
}

fn destination(value: &str) -> Result<SocketAddr> {
    let address = socket(value, "--target")?;
    if address.port() == 0 || address.ip().is_unspecified() || address.ip().is_multicast() {
        bail!("--target must be a specific unicast IP address with a non-zero port");
    }
    Ok(address)
}

/// Tracks one unauthenticated connection against the global and per-source
/// limits; the slot is released when the guard drops.
struct PendingGuard {
    _permit: tokio::sync::OwnedSemaphorePermit,
    per_ip: Arc<std::sync::Mutex<std::collections::HashMap<std::net::IpAddr, usize>>>,
    ip: std::net::IpAddr,
}

impl PendingGuard {
    fn try_new(
        pending: &Arc<Semaphore>,
        per_ip: &Arc<std::sync::Mutex<std::collections::HashMap<std::net::IpAddr, usize>>>,
        ip: std::net::IpAddr,
    ) -> Option<Self> {
        let permit = pending.clone().try_acquire_owned().ok()?;
        let mut map = per_ip.lock().unwrap_or_else(|e| e.into_inner());
        let count = map.entry(ip).or_insert(0);
        if *count >= MAX_PENDING_PER_IP {
            return None;
        }
        *count += 1;
        drop(map);
        Some(Self {
            _permit: permit,
            per_ip: per_ip.clone(),
            ip,
        })
    }
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        let mut map = self.per_ip.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(count) = map.get_mut(&self.ip) {
            *count -= 1;
            if *count == 0 {
                map.remove(&self.ip);
            }
        }
    }
}

async fn pump_with_idle_timeout<R, W>(
    mut reader: R,
    mut writer: W,
    idle: Duration,
) -> std::io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        let read = timeout(idle, reader.read(&mut buffer))
            .await
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "stream idle timeout")
            })??;
        if read == 0 {
            writer.shutdown().await?;
            return Ok(());
        }
        writer.write_all(&buffer[..read]).await?;
    }
}

/// Relay both directions until each side closes, failing a direction that is idle too long.
async fn relay_with_idle_timeout<A, B>(a: A, b: B, idle: Duration) -> std::io::Result<()>
where
    A: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    B: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (a_read, a_write) = tokio::io::split(a);
    let (b_read, b_write) = tokio::io::split(b);
    tokio::try_join!(
        pump_with_idle_timeout(a_read, b_write, idle),
        pump_with_idle_timeout(b_read, a_write, idle)
    )?;
    Ok(())
}

fn next_backoff(current: Duration) -> Duration {
    current.saturating_mul(2).min(MAX_BACKOFF)
}

pub async fn serve(options: ServerOptions) -> Result<()> {
    let listen = socket(&options.listen, "--listen")?;
    let target = destination(&options.target)?;
    let engine: Arc<dyn StreamEngine> = match &options.noise {
        Some(noise) => Arc::new(NoiseEngine::from_settings(
            noise,
            &options.peer_id,
            &options.local_id,
            &options.tunnel_id,
            options.direction,
        )?),
        None => {
            let acceptor =
                tls::server_config(&options.ca, &options.crl, &options.cert, &options.key)?;
            Arc::new(TlsEngine::listener(
                acceptor,
                options.peer_id.clone(),
                options.peer_fingerprint.clone(),
            ))
        }
    };
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("bind {listen}"))?;
    let sessions = Arc::new(Semaphore::new(MAX_SESSIONS));
    let pending = Arc::new(Semaphore::new(MAX_PENDING_HANDSHAKES));
    let pending_per_ip = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let streams = Arc::new(Semaphore::new(options.max_streams));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut tasks = JoinSet::new();
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    eprintln!(
        "BAFT-TUNNEL server listening on {listen} (engine {})",
        engine.id()
    );

    loop {
        while tasks.try_join_next().is_some() {}
        tokio::select! {
            _ = &mut shutdown => {
                eprintln!("shutdown requested; stopping new peer sessions");
                break;
            }
            accepted = listener.accept() => {
                let (socket, remote) = match accepted {
                    Ok(pair) => pair,
                    Err(error) => {
                        // Transient accept failures (EMFILE, aborted handshakes) must not stop the server.
                        eprintln!("accept failed: {error}");
                        tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
                        continue;
                    }
                };
                let Some(pending_guard) = PendingGuard::try_new(&pending, &pending_per_ip, remote.ip()) else {
                    eprintln!("connection rejected: unauthenticated connection limit reached");
                    drop(socket);
                    continue;
                };
                let sessions = sessions.clone();
                let engine = engine.clone();
                let (local, peer, tunnel, direction, target, max, stream_slots) = (
                    options.local_id.clone(),
                    options.peer_id.clone(),
                    options.tunnel_id.clone(),
                    options.direction,
                    target,
                    options.max_streams,
                    streams.clone(),
                );
                let session_shutdown = shutdown_rx.clone();
                tasks.spawn(async move {
                    let result: Result<()> = async {
                        let mut secure = engine.accept(socket).await?;
                        timeout(
                            HANDSHAKE_TIMEOUT,
                            handshake::server_handshake(&mut secure, &local, &peer, &tunnel, direction),
                        )
                        .await
                        .context("protocol handshake timeout")??;
                        // Authenticated: release the pre-auth slot, then take an established-session slot.
                        drop(pending_guard);
                        let _session_permit = sessions
                            .try_acquire_owned()
                            .map_err(|_| anyhow::anyhow!("session capacity reached"))?;
                        server_session(secure, target, max, stream_slots, tunnel, direction, session_shutdown).await;
                        Ok(())
                    }
                    .await;
                    if let Err(error) = result {
                        eprintln!("peer session from {remote} closed: {error:#}");
                    }
                });
            }
        }
    }

    let _ = shutdown_tx.send(true);
    let drain = async { while tasks.join_next().await.is_some() {} };
    if timeout(STREAM_DRAIN_TIMEOUT, drain).await.is_err() {
        eprintln!("graceful peer-session drain timed out; aborting remaining sessions");
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
    eprintln!("BAFT-TUNNEL server stopped");
    Ok(())
}

async fn drain_stream_tasks(tasks: &mut JoinSet<()>, role: &str) {
    if tasks.is_empty() {
        return;
    }
    let drain = async { while tasks.join_next().await.is_some() {} };
    if timeout(SHUTDOWN_TIMEOUT, drain).await.is_err() {
        eprintln!("{role} stream tasks exceeded shutdown deadline; aborting remaining tasks");
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
}

async fn server_session<S>(
    stream: S,
    target: SocketAddr,
    max: usize,
    slots: Arc<Semaphore>,
    tunnel: String,
    direction: ConnectionDirection,
    mut shutdown: watch::Receiver<bool>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut session = Session::new_server(stream, yamux_config(max));
    let mut control = session.control();
    let mut stream_tasks = JoinSet::new();
    loop {
        let item = tokio::select! {
            item = session.next() => item,
            completed = stream_tasks.join_next(), if !stream_tasks.is_empty() => {
                let _ = completed;
                continue;
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    let close = tokio::spawn(async move { control.close().await });
                    while session.next().await.is_some() {}
                    let _ = close.await;
                    break;
                }
                continue;
            }
        };
        let Some(item) = item else {
            break;
        };
        match item {
            Ok(mut logical) => {
                eprintln!("yamux stream accepted; connecting fixed destination");
                let permit = match slots.clone().try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        drop(logical);
                        continue;
                    }
                };
                let destination = target;
                let stream_tunnel = tunnel.clone();
                stream_tasks.spawn(async move {
                    let _permit = permit;
                    let result: Result<()> = async {
                        timeout(
                            HANDSHAKE_TIMEOUT,
                            handshake::server_stream_open(&mut logical, &stream_tunnel, direction),
                        )
                        .await
                        .context("stream authorization timeout")??;
                        let mut upstream =
                            timeout(CONNECT_TIMEOUT, TcpStream::connect(destination))
                                .await
                                .context("destination connect timeout")??;
                        eprintln!("fixed destination connected for yamux stream");
                        relay_with_idle_timeout(&mut logical, &mut upstream, STREAM_IDLE_TIMEOUT)
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
    drain_stream_tasks(&mut stream_tasks, "server").await;
}

async fn connect_peer(options: &ForwardOptions) -> Result<Session<BoxedSecureStream>> {
    let remote = socket(&options.remote, "--remote")?;
    // Rebuilt on every (re)connect so a refreshed CRL on disk is picked up.
    let engine: Box<dyn StreamEngine> = match &options.noise {
        Some(noise) => Box::new(NoiseEngine::from_settings(
            noise,
            &options.local_id,
            &options.peer_id,
            &options.tunnel_id,
            options.direction,
        )?),
        None => Box::new(TlsEngine::dialer(
            tls::connector(tls::client_config(
                &options.ca,
                &options.crl,
                &options.cert,
                &options.key,
            )?),
            tls::server_name(&options.server_name)?,
            options.peer_id.clone(),
            options.peer_fingerprint.clone(),
        )),
    };
    let mut secure = engine.dial(remote).await?;
    timeout(
        HANDSHAKE_TIMEOUT,
        handshake::client_handshake(
            &mut secure,
            &options.local_id,
            &options.peer_id,
            &options.tunnel_id,
            options.direction,
        ),
    )
    .await
    .context("protocol handshake timeout")??;

    Ok(Session::new_client(
        secure,
        yamux_config(options.max_streams),
    ))
}

async fn run_forward_session(
    mut session: Session<BoxedSecureStream>,
    receiver: &mut Receiver<TcpStream>,
    slots: &Arc<Semaphore>,
    direction: ConnectionDirection,
    tunnel: &str,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let mut control = session.control();
    let mut stream_tasks = JoinSet::new();
    let mut driver = tokio::spawn(async move {
        while let Some(item) = session.next().await {
            match item {
                Ok(unexpected) => drop(unexpected),
                Err(error) => return Err(anyhow::anyhow!("yamux session failed: {error}")),
            }
        }
        Err(anyhow::anyhow!("yamux session closed"))
    });

    loop {
        tokio::select! {
            completed = stream_tasks.join_next(), if !stream_tasks.is_empty() => {
                let _ = completed;
            }
            local = receiver.recv() => {
                let Some(mut local) = local else {
                    control.close().await;
                    match timeout(STREAM_DRAIN_TIMEOUT, &mut driver).await {
                        Ok(Ok(Err(error))) if error.to_string() == "yamux session closed" => {},
                        Ok(Ok(Ok(()))) => {},
                        Ok(Ok(Err(error))) => eprintln!("session closed during shutdown: {error:#}"),
                        Ok(Err(error)) => eprintln!("session driver failed during shutdown: {error}"),
                        Err(_) => {
                            eprintln!("session driver did not stop before shutdown deadline; aborting it");
                            driver.abort();
                            let _ = driver.await;
                        }
                    }
                    drain_stream_tasks(&mut stream_tasks, "forwarder").await;
                    return Ok(());
                };
                let permit = match slots.clone().try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        drop(local);
                        continue;
                    }
                };
                let mut stream_control = control.clone();
                let tunnel = tunnel.to_owned();
                stream_tasks.spawn(async move {
                    let _permit = permit;
                    let result: Result<()> = async {
                        let mut stream = timeout(
                            CONNECT_TIMEOUT,
                            stream_control.open_stream(),
                        )
                        .await
                        .context("opening multiplexed stream timed out")?
                        .context("open multiplexed stream")?;
                        eprintln!("yamux stream opened for local connection");
                        handshake::client_stream_open(
                            &mut stream,
                            &tunnel,
                            direction,
                            StreamCapability::TcpForward,
                        )
                        .await
                        .context("authorize TCP forwarding stream")?;
                        relay_with_idle_timeout(&mut local, &mut stream, STREAM_IDLE_TIMEOUT)
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
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    control.close().await;
                    if timeout(STREAM_DRAIN_TIMEOUT, &mut driver).await.is_err() {
                        driver.abort();
                        let _ = driver.await;
                    }
                    drain_stream_tasks(&mut stream_tasks, "forwarder").await;
                    return Ok(());
                }
            }
            result = &mut driver => {
                let outcome = match result {
                    Ok(Err(error)) => Err(error),
                    Ok(Ok(())) => Err(anyhow::anyhow!("yamux session ended unexpectedly")),
                    Err(error) => Err(anyhow::anyhow!("yamux driver task failed: {error}")),
                };
                control.close().await;
                drain_stream_tasks(&mut stream_tasks, "forwarder").await;
                return outcome;
            }
        }
    }
}

async fn supervise_forward(
    options: ForwardOptions,
    mut receiver: Receiver<TcpStream>,
    slots: Arc<Semaphore>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut backoff = INITIAL_BACKOFF;
    loop {
        if receiver.is_closed() || *shutdown.borrow() {
            break;
        }
        let connection = tokio::select! {
            result = connect_peer(&options) => result,
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break; }
                continue;
            }
        };
        match connection {
            Ok(session) => {
                eprintln!("authenticated tunnel session established");
                backoff = INITIAL_BACKOFF;
                if let Err(error) = run_forward_session(
                    session,
                    &mut receiver,
                    &slots,
                    options.direction,
                    &options.tunnel_id,
                    shutdown.clone(),
                )
                .await
                {
                    if !receiver.is_closed() {
                        eprintln!("tunnel session lost: {error:#}");
                    }
                }
            }
            Err(error) => {
                if receiver.is_closed() {
                    break;
                }
                eprintln!("tunnel connection attempt failed: {error:#}");
            }
        }

        if receiver.is_closed() {
            break;
        }
        let jitter_ms = u64::from(OsRng.next_u32() % 500);
        tokio::select! {
            _ = sleep(backoff + Duration::from_millis(jitter_ms)) => {},
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break; }
            }
        }
        backoff = next_backoff(backoff);
    }
}

pub async fn forward(options: ForwardOptions) -> Result<()> {
    let listen = socket(&options.listen, "--listen")?;
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("bind {listen}"))?;
    let (sender, receiver) = mpsc::channel(options.max_streams);
    let slots = Arc::new(Semaphore::new(options.max_streams));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let supervisor = tokio::spawn(supervise_forward(options, receiver, slots, shutdown_rx));
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

    eprintln!("BAFT-TUNNEL local forwarder listening on {listen}");
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                eprintln!("shutdown requested; stopping new local connections");
                break;
            }
            accepted = listener.accept() => {
                let (local, _) = match accepted {
                    Ok(pair) => pair,
                    Err(error) => {
                        eprintln!("local accept failed: {error}");
                        tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
                        continue;
                    }
                };
                if sender.try_send(local).is_err() {
                    eprintln!("local connection rejected: bounded pending queue is full");
                }
            }
        }
    }
    let _ = shutdown_tx.send(true);
    drop(sender);
    let mut supervisor = supervisor;
    if timeout(SHUTDOWN_TIMEOUT, &mut supervisor).await.is_err() {
        eprintln!("forwarder supervisor did not stop before shutdown deadline; aborting it");
        supervisor.abort();
        let _ = supervisor.await;
    }
    eprintln!("BAFT-TUNNEL forwarder stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

    #[test]
    fn destination_requires_specific_unicast_socket() {
        assert!(destination("127.0.0.1:8080").is_ok());
        assert!(destination("0.0.0.0:8080").is_err());
        assert!(destination("224.0.0.1:8080").is_err());
        assert!(destination("127.0.0.1:0").is_err());
        assert!(destination("example.invalid:8080").is_err());
    }

    #[test]
    fn reconnect_backoff_is_exponential_and_bounded() {
        let mut delay = INITIAL_BACKOFF;
        for _ in 0..6 {
            delay = next_backoff(delay);
        }
        assert_eq!(delay, MAX_BACKOFF);
        assert_eq!(next_backoff(delay), MAX_BACKOFF);
    }

    async fn authenticated_forwarding_case(client_id: &str, server_id: &str) {
        use rcgen::{
            BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
            KeyUsagePurpose, SanType, string::Ia5String,
        };
        use sha2::{Digest, Sha256};
        use std::{
            fs,
            time::{SystemTime, UNIX_EPOCH},
        };

        let directory = std::env::temp_dir().join(format!(
            "baft-tunnel-e2e-{}-{}",
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
        server_params.subject_alt_names.push(SanType::URI(
            Ia5String::try_from(format!("urn:baft-tunnel:node:{server_id}")).unwrap(),
        ));
        server_params.use_authority_key_identifier_extension = true;
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
        client_params.subject_alt_names.push(SanType::URI(
            Ia5String::try_from(format!("urn:baft-tunnel:node:{client_id}")).unwrap(),
        ));
        client_params.use_authority_key_identifier_extension = true;
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

        let echo_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_address = echo_listener.local_addr().unwrap();
        let echo_task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = echo_listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 1024];
                    loop {
                        let Ok(count) = stream.read(&mut buffer).await else {
                            break;
                        };
                        if count == 0 || stream.write_all(&buffer[..count]).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });

        let server_reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_address = server_reservation.local_addr().unwrap();
        drop(server_reservation);
        let local_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_address = local_listener.local_addr().unwrap();

        let direction = if client_id == "iran" {
            ConnectionDirection::Direct
        } else {
            ConnectionDirection::Reverse
        };
        let server_task = tokio::spawn(serve(ServerOptions {
            listen: server_address.to_string(),
            target: echo_address.to_string(),
            local_id: server_id.to_owned(),
            peer_id: client_id.to_owned(),
            tunnel_id: "e2e-tunnel".to_owned(),
            direction,
            peer_fingerprint: client_pin,
            ca: ca_path.clone(),
            crl: crl_path.clone(),
            cert: server_cert_path,
            key: server_key_path,
            noise: None,
            max_streams: 8,
        }));

        let mut ready = false;
        for _ in 0..100 {
            if TcpStream::connect(server_address).await.is_ok() {
                ready = true;
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
        assert!(ready, "test server did not bind its TCP listener");

        let forward_options = ForwardOptions {
            listen: local_address.to_string(),
            remote: server_address.to_string(),
            server_name: "localhost".to_owned(),
            local_id: client_id.to_owned(),
            peer_id: server_id.to_owned(),
            tunnel_id: "e2e-tunnel".to_owned(),
            direction,
            peer_fingerprint: server_pin,
            ca: ca_path,
            crl: crl_path,
            cert: client_cert_path,
            key: client_key_path,
            noise: None,
            max_streams: 8,
        };
        let session = timeout(Duration::from_secs(10), connect_peer(&forward_options))
            .await
            .expect("authenticated peer connection timed out")
            .expect("authenticated peer connection failed");

        let (sender, mut receiver) = mpsc::channel(8);
        let stream_slots = Arc::new(Semaphore::new(8));
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let forward_task = tokio::spawn(async move {
            run_forward_session(
                session,
                &mut receiver,
                &stream_slots,
                direction,
                "e2e-tunnel",
                shutdown_rx,
            )
            .await
        });

        let mut local_client = timeout(Duration::from_secs(5), TcpStream::connect(local_address))
            .await
            .expect("local test client connection timed out")
            .expect("local test client connection failed");
        let (local_stream, _) = local_listener.accept().await.unwrap();
        sender.send(local_stream).await.unwrap();

        // Exercise flow-control updates beyond the default Yamux initial window.
        let payload: Vec<u8> = (0..(2 * 1024 * 1024 + 137))
            .map(|index| (index % 251) as u8)
            .collect();
        timeout(Duration::from_secs(10), async {
            local_client.write_all(&payload).await.unwrap();
            let mut received = vec![0_u8; payload.len()];
            local_client.read_exact(&mut received).await.unwrap();
            assert_eq!(received, payload);
        })
        .await
        .expect("end-to-end forwarded payload timed out");

        drop(local_client);
        forward_task.abort();
        server_task.abort();
        echo_task.abort();
        let _ = forward_task.await;
        let _ = server_task.await;
        let _ = echo_task.await;
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn authenticated_tcp_tls_forwarding_works_with_each_side_as_dialer() {
        authenticated_forwarding_case("iran", "abroad").await;
        authenticated_forwarding_case("abroad", "iran").await;
    }

    #[tokio::test]
    async fn noise_tcp_forwarding_works_end_to_end_and_rejects_wrong_peer_key() {
        let directory = std::env::temp_dir().join(format!(
            "baft-noise-e2e-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let server_key = directory.join("server.noise");
        let client_key = directory.join("client.noise");
        let rogue_key = directory.join("rogue.noise");
        let server_public = crate::engine::write_noise_keypair(&server_key).unwrap();
        let client_public = crate::engine::write_noise_keypair(&client_key).unwrap();
        let rogue_public = crate::engine::write_noise_keypair(&rogue_key).unwrap();

        let echo_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_address = echo_listener.local_addr().unwrap();
        let echo_task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = echo_listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 1024];
                    while let Ok(count) = stream.read(&mut buffer).await {
                        if count == 0 || stream.write_all(&buffer[..count]).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_address = reservation.local_addr().unwrap();
        drop(reservation);
        let local_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_address = local_listener.local_addr().unwrap();

        let direction = ConnectionDirection::Direct;
        let server_task = tokio::spawn(serve(ServerOptions {
            listen: server_address.to_string(),
            target: echo_address.to_string(),
            local_id: "abroad".to_owned(),
            peer_id: "iran".to_owned(),
            tunnel_id: "noise-e2e".to_owned(),
            direction,
            peer_fingerprint: String::new(),
            ca: PathBuf::new(),
            crl: PathBuf::new(),
            cert: PathBuf::new(),
            key: server_key.clone(),
            noise: Some(NoiseSettings {
                private_key: server_key,
                peer_public: client_public,
            }),
            max_streams: 8,
        }));
        for _ in 0..100 {
            if TcpStream::connect(server_address).await.is_ok() {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
        let options = |private: PathBuf, peer_public: String| ForwardOptions {
            listen: local_address.to_string(),
            remote: server_address.to_string(),
            server_name: String::new(),
            local_id: "iran".to_owned(),
            peer_id: "abroad".to_owned(),
            tunnel_id: "noise-e2e".to_owned(),
            direction,
            peer_fingerprint: String::new(),
            ca: PathBuf::new(),
            crl: PathBuf::new(),
            cert: PathBuf::new(),
            key: private.clone(),
            noise: Some(NoiseSettings {
                private_key: private,
                peer_public,
            }),
            max_streams: 8,
        };

        // A dialer that pins the wrong server key must not obtain a session.
        let wrong = timeout(
            Duration::from_secs(10),
            connect_peer(&options(client_key.clone(), rogue_public)),
        )
        .await
        .expect("wrong-key attempt timed out");
        assert!(wrong.is_err(), "dialer accepted an unpinned server key");
        // A dialer using an unenrolled static key must be refused by the server.
        let unenrolled = timeout(
            Duration::from_secs(10),
            connect_peer(&options(rogue_key, server_public.clone())),
        )
        .await
        .expect("unenrolled-key attempt timed out");
        assert!(
            unenrolled.is_err(),
            "server accepted an unenrolled client key"
        );

        let session = timeout(
            Duration::from_secs(10),
            connect_peer(&options(client_key, server_public)),
        )
        .await
        .expect("noise peer connection timed out")
        .expect("noise peer connection failed");
        let (sender, mut receiver) = mpsc::channel(8);
        let stream_slots = Arc::new(Semaphore::new(8));
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let forward_task = tokio::spawn(async move {
            run_forward_session(
                session,
                &mut receiver,
                &stream_slots,
                direction,
                "noise-e2e",
                shutdown_rx,
            )
            .await
        });
        let mut local_client = TcpStream::connect(local_address).await.unwrap();
        let (local_stream, _) = local_listener.accept().await.unwrap();
        sender.send(local_stream).await.unwrap();
        let payload: Vec<u8> = (0..(512 * 1024 + 29)).map(|i| (i % 253) as u8).collect();
        timeout(Duration::from_secs(10), async {
            local_client.write_all(&payload).await.unwrap();
            let mut received = vec![0_u8; payload.len()];
            local_client.read_exact(&mut received).await.unwrap();
            assert_eq!(received, payload);
        })
        .await
        .expect("noise forwarded payload timed out");

        drop(local_client);
        forward_task.abort();
        server_task.abort();
        echo_task.abort();
        let _ = forward_task.await;
        let _ = server_task.await;
        let _ = echo_task.await;
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn yamux_multiplexes_concurrent_streams_without_corrupting_bytes() {
        let (client_io, server_io) = duplex(1024 * 1024);
        let mut server_session = Session::new_server(server_io, yamux_config(8));
        let server_task = tokio::spawn(async move {
            while let Some(item) = server_session.next().await {
                match item {
                    Ok(mut stream) => {
                        tokio::spawn(async move {
                            let mut payload = vec![0_u8; 4096];
                            if stream.read_exact(&mut payload).await.is_ok() {
                                let _ = stream.write_all(&payload).await;
                            }
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        let mut client_session = Session::new_client(client_io, yamux_config(8));
        let control = client_session.control();
        let client_task = tokio::spawn(async move {
            while let Some(item) = client_session.next().await {
                if item.is_err() {
                    break;
                }
            }
        });

        let mut tasks = Vec::new();
        for stream_id in 0_u8..8 {
            let mut stream_control = control.clone();
            tasks.push(tokio::spawn(async move {
                let mut stream = stream_control.open_stream().await.unwrap();
                let expected = vec![stream_id; 4096];
                stream.write_all(&expected).await.unwrap();
                let mut received = vec![0_u8; 4096];
                stream.read_exact(&mut received).await.unwrap();
                assert_eq!(received, expected);
            }));
        }

        for task in tasks {
            timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
        }
        server_task.abort();
        client_task.abort();
    }

    #[test]
    fn pending_guard_limits_per_source_and_releases_slots() {
        let pending = Arc::new(Semaphore::new(MAX_PENDING_HANDSHAKES));
        let per_ip = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let a: std::net::IpAddr = "198.51.100.1".parse().unwrap();
        let b: std::net::IpAddr = "198.51.100.2".parse().unwrap();
        let g1 = PendingGuard::try_new(&pending, &per_ip, a).unwrap();
        let _g2 = PendingGuard::try_new(&pending, &per_ip, a).unwrap();
        assert!(PendingGuard::try_new(&pending, &per_ip, a).is_none());
        // Another source is unaffected and the rejected attempt leaked no global slot.
        assert!(PendingGuard::try_new(&pending, &per_ip, b).is_some());
        assert_eq!(pending.available_permits(), MAX_PENDING_HANDSHAKES - 2);
        drop(g1);
        assert!(PendingGuard::try_new(&pending, &per_ip, a).is_some());
    }

    #[test]
    fn pending_guard_enforces_global_limit_independent_of_sessions() {
        let pending = Arc::new(Semaphore::new(MAX_PENDING_HANDSHAKES));
        let per_ip = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let mut guards = Vec::new();
        for n in 0..MAX_PENDING_HANDSHAKES {
            let ip: std::net::IpAddr = format!("203.0.113.{n}").parse().unwrap();
            guards.push(PendingGuard::try_new(&pending, &per_ip, ip).unwrap());
        }
        let extra: std::net::IpAddr = "192.0.2.1".parse().unwrap();
        assert!(PendingGuard::try_new(&pending, &per_ip, extra).is_none());
    }

    #[tokio::test]
    async fn relay_closes_idle_stream_and_passes_data_both_ways() {
        let (mut client, relay_a) = duplex(1024);
        let (relay_b, mut server) = duplex(1024);
        let task = tokio::spawn(relay_with_idle_timeout(
            relay_a,
            relay_b,
            Duration::from_millis(300),
        ));
        client.write_all(b"ping").await.unwrap();
        let mut buffer = [0u8; 4];
        server.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, b"ping");
        server.write_all(b"pong").await.unwrap();
        client.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, b"pong");
        // No more traffic: the relay must give up instead of holding the slot forever.
        let result = timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
    }
}
