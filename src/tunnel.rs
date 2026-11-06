use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use crate::{handshake, tls};
use anyhow::{Context, Result, bail};
use futures::StreamExt;
use rand::{RngCore, rngs::OsRng};
use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
    sync::{
        Semaphore,
        mpsc::{self, Receiver},
    },
    time::{sleep, timeout},
};
use tokio_rustls::client::TlsStream as ClientTlsStream;
use tokio_yamux::{Config as YamuxConfig, Session};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_SESSIONS: usize = 8;
// Keep aggregate receive buffering bounded while allowing a 2 MiB payload
// to pass without depending on repeated window-update cycles over high-RTT links.
const MAX_STREAM_WINDOW_SIZE: u32 = 4 * 1024 * 1024;
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

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

fn next_backoff(current: Duration) -> Duration {
    current.saturating_mul(2).min(MAX_BACKOFF)
}

pub async fn serve(options: ServerOptions) -> Result<()> {
    let listen = socket(&options.listen, "--listen")?;
    let target = destination(&options.target)?;
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
            target,
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
                handshake::verify_peer_identity(secure.get_ref().1.peer_certificates(), &peer)?;
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

async fn server_session<S>(stream: S, target: SocketAddr, max: usize, slots: Arc<Semaphore>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut session = Session::new_server(stream, yamux_config(max));
    while let Some(item) = session.next().await {
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
                tokio::spawn(async move {
                    let _permit = permit;
                    let result: Result<()> = async {
                        let mut upstream =
                            timeout(CONNECT_TIMEOUT, TcpStream::connect(destination))
                                .await
                                .context("destination connect timeout")??;
                        eprintln!("fixed destination connected for yamux stream");
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

async fn connect_peer(options: &ForwardOptions) -> Result<Session<ClientTlsStream<TcpStream>>> {
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
    handshake::verify_peer_identity(secure.get_ref().1.peer_certificates(), &options.peer_id)?;
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

    Ok(Session::new_client(
        secure,
        yamux_config(options.max_streams),
    ))
}

async fn run_forward_session(
    mut session: Session<ClientTlsStream<TcpStream>>,
    receiver: &mut Receiver<TcpStream>,
    slots: &Arc<Semaphore>,
) -> Result<()> {
    let control = session.control();
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
            local = receiver.recv() => {
                let Some(mut local) = local else {
                    bail!("local connection queue closed");
                };
                let permit = match slots.clone().try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        drop(local);
                        continue;
                    }
                };
                let mut stream_control = control.clone();
                tokio::spawn(async move {
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
            result = &mut driver => {
                match result {
                    Ok(Err(error)) => return Err(error),
                    Ok(Ok(())) => bail!("yamux session ended unexpectedly"),
                    Err(error) => bail!("yamux driver task failed: {error}"),
                }
            }
        }
    }
}

async fn supervise_forward(
    options: ForwardOptions,
    mut receiver: Receiver<TcpStream>,
    slots: Arc<Semaphore>,
) {
    let mut backoff = INITIAL_BACKOFF;
    loop {
        match connect_peer(&options).await {
            Ok(session) => {
                eprintln!("authenticated tunnel session established");
                backoff = INITIAL_BACKOFF;
                if let Err(error) = run_forward_session(session, &mut receiver, &slots).await {
                    eprintln!("tunnel session lost: {error:#}");
                }
            }
            Err(error) => {
                eprintln!("tunnel connection attempt failed: {error:#}");
            }
        }

        let jitter_ms = u64::from(OsRng.next_u32() % 500);
        sleep(backoff + Duration::from_millis(jitter_ms)).await;
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
    tokio::spawn(supervise_forward(options, receiver, slots));

    eprintln!("BAFT-TUNNEL local forwarder listening on {listen}");
    loop {
        let (local, _) = listener.accept().await?;
        if sender.try_send(local).is_err() {
            eprintln!("local connection rejected: bounded pending queue is full");
        }
    }
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
        let server_cert_path = directory.join("server.pem");
        let server_key_path = directory.join("server.key");
        let client_cert_path = directory.join("client.pem");
        let client_key_path = directory.join("client.key");
        fs::write(&ca_path, ca_cert.pem()).unwrap();
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

        let server_task = tokio::spawn(serve(ServerOptions {
            listen: server_address.to_string(),
            target: echo_address.to_string(),
            local_id: server_id.to_owned(),
            peer_id: client_id.to_owned(),
            tunnel_id: "e2e-tunnel".to_owned(),
            peer_fingerprint: client_pin,
            ca: ca_path.clone(),
            cert: server_cert_path,
            key: server_key_path,
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
            peer_fingerprint: server_pin,
            ca: ca_path,
            cert: client_cert_path,
            key: client_key_path,
            max_streams: 8,
        };
        let session = timeout(Duration::from_secs(10), connect_peer(&forward_options))
            .await
            .expect("authenticated peer connection timed out")
            .expect("authenticated peer connection failed");

        let (sender, mut receiver) = mpsc::channel(8);
        let stream_slots = Arc::new(Semaphore::new(8));
        let forward_task = tokio::spawn(async move {
            run_forward_session(session, &mut receiver, &stream_slots).await
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
}
