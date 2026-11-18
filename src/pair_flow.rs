//! Automatic pairing orchestration (ADR-0050, stage 4): `pair offer` on the listening node and
//! `pair accept CODE` on the other node. Reuses `setup::init`/`setup::join`, so the resulting
//! configuration is exactly what guided setup produces; only the way the PKI files and the pairing
//! bundles reach the nodes changes. Direct, `tcp-tls`, protocol v2 (Reverse is a later stage).

use std::{
    fs,
    future::Future,
    net::{SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::{Mutex, oneshot},
    time::timeout,
};

use crate::{
    authorization::ConnectionDirection,
    config::EngineKind,
    pair_ca::{self, Member},
    pair_proto::{self, JoinRequest, JoinResponse, PairCode},
    setup::{self, Bundle, PlanRequest, Role},
};

/// Where this server is, as the user answered it. Used for defaults only; reachability is always
/// checked by the join itself, never assumed from the answer (ADR-0050 Addendum 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Location {
    Iran,
    Abroad,
}

impl std::str::FromStr for Location {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "iran" => Ok(Self::Iran),
            "abroad" => Ok(Self::Abroad),
            _ => bail!("location must be iran or abroad"),
        }
    }
}

/// Tunnel direction chosen at offer time. Reverse is the tcp-tls Preview (ADR-0021/0037): the
/// listener is the public node (user ingress), the joiner is the service node that dials out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Direct,
    Reverse,
}

impl Direction {
    fn label(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Reverse => "reverse",
        }
    }
}

impl std::str::FromStr for Direction {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "direct" => Ok(Self::Direct),
            "reverse" => Ok(Self::Reverse),
            _ => bail!("direction must be direct or reverse"),
        }
    }
}

/// Session rotation default for a Direct dialer inside Iran (ADR-0049; aged-flow throttle).
const IRAN_ROTATE_SECS: u64 = 10;

/// Add `session_rotate_secs` to the generated forward config and prove the runtime accepts it.
fn set_rotation(cfg: &Path, secs: u64) -> Result<()> {
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(cfg)?)?;
    v.as_object_mut()
        .context("configuration is not an object")?
        .insert("session_rotate_secs".into(), secs.into());
    fs::write(cfg, serde_json::to_vec_pretty(&v)?)?;
    crate::config::load(cfg)
        .context("the rotation default is not accepted by this configuration")?;
    Ok(())
}

pub struct OfferOptions {
    pub dir: PathBuf,
    pub instance: Option<String>,
    /// Human name; becomes the tunnel id and prefix of the generated node ids.
    pub name: String,
    pub service_port: u16,
    pub tunnel_port: u16,
    /// Address the other node dials; detected when absent.
    pub host: Option<String>,
    pub direction: Direction,
}

fn slug(name: &str) -> Result<String> {
    let s: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    ensure!(
        !s.is_empty() && s.len() <= 32,
        "name must be 1-32 letters, digits or dashes"
    );
    Ok(s)
}

fn short() -> String {
    use rand::RngCore;
    let mut b = [0u8; 3];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

/// The address this host uses to reach the internet (no packet is sent).
pub fn detect_host() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}

fn gen_key_and_csr(dir: &Path, node_id: &str) -> Result<String> {
    let key = pair_ca::run(
        &[
            "genpkey",
            "-algorithm",
            "EC",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
        ],
        None,
    )?;
    let key_path = dir.join("node.key");
    pair_ca::write_private(&key_path, &key, 0o600)?;
    let csr = pair_ca::run(
        &[
            "req",
            "-new",
            "-key",
            &key_path.to_string_lossy(),
            "-subj",
            &format!("/CN={node_id}"),
        ],
        None,
    )?;
    Ok(String::from_utf8(csr)?)
}

fn install_public(dir: &Path, leaf: &str, ca: &str, crl: &str) -> Result<()> {
    for (name, body) in [("node.crt", leaf), ("ca.crt", ca), ("ca.crl", crl)] {
        pair_ca::write_private(&dir.join(name), body.as_bytes(), 0o644)?;
    }
    Ok(())
}

fn req(
    dir: &Path,
    instance: &Option<String>,
    role: Role,
    node: &str,
    peer: &str,
    code: &PairCode,
) -> PlanRequest {
    PlanRequest {
        engine: EngineKind::TcpTls,
        role,
        direction: if code.direction == "reverse" {
            ConnectionDirection::Reverse
        } else {
            ConnectionDirection::Direct
        },
        protocol_version: setup::default_protocol_for(EngineKind::TcpTls),
        node_id: node.into(),
        peer_id: peer.into(),
        tunnel_id: code.tunnel_id.clone(),
        service_port: code.service_port,
        tunnel_port: code.tunnel_port,
        peer_host: (role == Role::Forward).then(|| code.host.clone()),
        backup_peer_hosts: vec![],
        server_name: None,
        dir: dir.to_path_buf(),
        instance: instance.clone(),
    }
}

fn adopt(dir: &Path, instance: &Option<String>) -> Result<()> {
    if let Some(name) = instance {
        for line in setup::adopt_for_service(dir, &format!("{name}.json"))? {
            println!("Owner/mode set for the service: {line}");
        }
    }
    Ok(())
}

/// Files a pairing creates in the configuration directory; removed again when it fails.
const OWNED: [&str; 9] = [
    "pair-ca",
    "pair-join",
    "node.key",
    "node.crt",
    "ca.crt",
    "ca.crl",
    "bundle.json",
    "peer-bundle.json",
    "pairing.json",
];

fn precheck(dir: &Path, instance: &Option<String>) -> Result<()> {
    let mut names: Vec<String> = OWNED.iter().map(|s| s.to_string()).collect();
    names.push("config.json".into());
    if let Some(i) = instance {
        names.push(format!("{i}.json"));
    }
    for n in names {
        ensure!(
            fs::symlink_metadata(dir.join(&n)).is_err(),
            "{} already holds a configuration ({n}); pair into an empty configuration",
            dir.display()
        );
    }
    Ok(())
}

fn cleanup(dir: &Path, instance: &Option<String>) {
    for f in OWNED {
        let p = dir.join(f);
        if p.is_dir() {
            let _ = fs::remove_dir_all(&p);
        } else {
            let _ = fs::remove_file(&p);
        }
    }
    let _ = fs::remove_file(dir.join("config.json"));
    if let Some(i) = instance {
        let _ = fs::remove_file(dir.join(format!("{i}.json")));
    }
}

fn ensure_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o750)
            .create(dir)?;
    }
    Ok(())
}

// ---- privilege-separated network worker (ADR-0050, review H1) ---------------------------------
//
// The process that touches the network never sees the CA key. It receives the one-time token,
// serves the pairing exchange, and forwards the single verified join request to the parent, which
// alone holds the CA, asks the user and signs. When the helper runs as root the worker is a child
// process that drops to the service user before doing anything; otherwise it runs in-process.

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ToWorker {
    Start {
        port: u16,
        token: String,
        joiner_id: String,
        exp: u64,
    },
    Decision(JoinResponse),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FromWorker {
    Ready {
        pin: String,
    },
    Request {
        node_id: String,
        csr_pem: String,
        peer: String,
    },
    Done {
        ok: bool,
        error: Option<String>,
    },
}

const LINE_MAX: usize = 32 * 1024;

async fn send_line<W: AsyncWrite + Unpin>(w: &mut W, v: &impl Serialize) -> Result<()> {
    let mut line = serde_json::to_vec(v)?;
    line.push(b'\n');
    w.write_all(&line).await?;
    w.flush().await?;
    Ok(())
}

async fn read_line<R: AsyncBufRead + Unpin, T: for<'a> Deserialize<'a>>(
    r: &mut R,
) -> Result<Option<T>> {
    let mut buf = Vec::new();
    let n = r
        .take(LINE_MAX as u64 + 1)
        .read_until(b'\n', &mut buf)
        .await?;
    if n == 0 {
        return Ok(None);
    }
    ensure!(
        buf.len() <= LINE_MAX && buf.ends_with(b"\n"),
        "oversized or truncated line"
    );
    Ok(Some(serde_json::from_slice(&buf)?))
}

/// The network side. Never reads CA material; its only inputs are the start message and decisions.
pub async fn worker<R, W>(mut rd: R, wr: W) -> Result<()>
where
    R: AsyncBufRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let Some(ToWorker::Start {
        port,
        token,
        joiner_id,
        exp,
    }) = read_line(&mut rd).await?
    else {
        bail!("worker: expected a start message");
    };
    let scratch = std::env::temp_dir().join(format!("baft-pair-{:x}", rand::random::<u64>()));
    let identity = pair_proto::pairing_identity(&scratch);
    let _ = fs::remove_dir_all(&scratch);
    let (acceptor, pin) = identity?;
    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("cannot listen on port {port} (is it free?)"))?;
    let wr = Arc::new(Mutex::new(wr));
    send_line(
        &mut *wr.lock().await,
        &FromWorker::Ready { pin: pin.clone() },
    )
    .await?;

    let (dec_tx, dec_rx) = oneshot::channel::<JoinResponse>();
    tokio::spawn(async move {
        let mut dec_tx = Some(dec_tx);
        while let Ok(Some(msg)) = read_line::<_, ToWorker>(&mut rd).await {
            if let (ToWorker::Decision(d), Some(tx)) = (msg, dec_tx.take()) {
                let _ = tx.send(d);
            }
        }
    });
    let dec_rx = Arc::new(std::sync::Mutex::new(Some(dec_rx)));
    // Only token, joiner id and expiry are consulted by the server; the rest is a placeholder.
    let code = PairCode {
        v: 1,
        host: "0.0.0.0".into(),
        port,
        pin,
        token,
        tunnel_id: "worker".into(),
        direction: "direct".into(),
        listener_id: "worker".into(),
        joiner_id,
        service_port: 1024,
        tunnel_port: port,
        server_name: "IP:0.0.0.0".into(),
        exp,
    };
    let wr2 = wr.clone();
    let result = pair_proto::serve(listener, acceptor, &code, move |req: JoinRequest, peer| {
        let (wr, rx) = (wr2.clone(), dec_rx.lock().ok().and_then(|mut g| g.take()));
        async move {
            let rx = rx.context("a pairing request is already being decided")?;
            send_line(
                &mut *wr.lock().await,
                &FromWorker::Request {
                    node_id: req.node_id,
                    csr_pem: req.csr_pem,
                    peer: peer.to_string(),
                },
            )
            .await?;
            timeout(pair_proto::RESPONSE_DEADLINE, rx)
                .await
                .context("no decision in time")?
                .context("the controlling process went away")
        }
    })
    .await;
    let done = match result {
        Ok(_) => FromWorker::Done {
            ok: true,
            error: None,
        },
        Err(e) => FromWorker::Done {
            ok: false,
            error: Some(format!("{e:#}")),
        },
    };
    send_line(&mut *wr.lock().await, &done).await
}

/// Entry of the hidden `pair worker` subcommand: drop privileges, then serve over stdio.
pub async fn worker_stdio() -> Result<()> {
    drop_privileges()?;
    worker(BufReader::new(tokio::io::stdin()), tokio::io::stdout()).await
}

fn drop_privileges() -> Result<()> {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        return Ok(());
    }
    let (uid, gid) = setup::service_ids().context("the service user does not exist")?;
    // SAFETY: plain syscalls on this process; checked below. Supplementary groups, gid, then uid.
    unsafe {
        ensure!(
            libc::setgroups(0, std::ptr::null()) == 0,
            "setgroups failed"
        );
        ensure!(libc::setgid(gid) == 0, "setgid failed");
        ensure!(libc::setuid(uid) == 0, "setuid failed");
        ensure!(libc::setuid(0) != 0, "privileges could be regained");
        ensure!(
            libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0,
            "no-new-privileges failed"
        );
    }
    Ok(())
}

type Transport = (
    Box<dyn AsyncWrite + Unpin + Send>,
    Box<dyn AsyncBufRead + Unpin + Send>,
    Option<tokio::process::Child>,
);

fn start_worker() -> Result<Transport> {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        ensure!(
            setup::service_ids().is_some(),
            "the service user does not exist; run scripts/install.sh --binary first"
        );
        let mut child = tokio::process::Command::new(std::env::current_exe()?)
            .args(["pair", "worker"])
            .env_clear()
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("cannot start the pairing network worker")?;
        let stdin = child.stdin.take().context("worker stdin")?;
        let stdout = child.stdout.take().context("worker stdout")?;
        return Ok((
            Box::new(stdin),
            Box::new(BufReader::new(stdout)),
            Some(child),
        ));
    }
    let (p2w_tx, p2w_rx) = tokio::io::duplex(64 * 1024);
    let (w2p_tx, w2p_rx) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let _ = worker(BufReader::new(p2w_rx), w2p_tx).await;
    });
    Ok((Box::new(p2w_tx), Box::new(BufReader::new(w2p_rx)), None))
}

/// A joiner waiting for the listener's user to approve.
pub struct Pending {
    pub node_id: String,
    pub peer: SocketAddr,
    /// Short fingerprint of the joiner's key; the joiner shows the same string.
    pub verify: String,
}

/// Listening node: prepare everything, print the code, wait for one approved join, write config.
/// `approve` is asked once, for the single verified request; the listener never listens as root.
pub async fn offer<A, AF>(
    opts: OfferOptions,
    mut approve: A,
    announce: impl FnOnce(&str),
) -> Result<()>
where
    A: FnMut(Pending) -> AF,
    AF: Future<Output = bool>,
{
    ensure!(
        opts.service_port >= 1024 && opts.tunnel_port >= 1024,
        "ports must be 1024-65535"
    );
    let name = slug(&opts.name)?;
    let host = match opts.host.clone().or_else(detect_host) {
        Some(h) => h,
        None => bail!("cannot detect this server's address; pass --host"),
    };
    ensure_dir(&opts.dir)?;
    precheck(&opts.dir, &opts.instance)?;
    let r = offer_inner(&opts, &name, host, &mut approve, announce).await;
    if r.is_err() {
        cleanup(&opts.dir, &opts.instance);
    }
    r
}

async fn offer_inner<A, AF>(
    opts: &OfferOptions,
    name: &str,
    host: String,
    approve: &mut A,
    announce: impl FnOnce(&str),
) -> Result<()>
where
    A: FnMut(Pending) -> AF,
    AF: Future<Output = bool>,
{
    let server_name = if host.parse::<std::net::IpAddr>().is_ok() {
        format!("IP:{host}")
    } else {
        format!("DNS:{host}")
    };
    let (listener_id, joiner_id) = (
        format!("{name}-s-{}", short()),
        format!("{name}-f-{}", short()),
    );
    let members = [
        Member {
            node_id: listener_id.clone(),
            san: server_name.clone(),
        },
        Member {
            node_id: joiner_id.clone(),
            san: format!("DNS:{joiner_id}.pair.invalid"),
        },
    ];
    let dir = &opts.dir;
    let ca_dir = dir.join("pair-ca");
    let ca_pem = pair_ca::create(&ca_dir, &format!("BAFT pair {name}"), &members)?;
    let csr = gen_key_and_csr(dir, &listener_id)?;
    let leaf = pair_ca::sign(&ca_dir, &listener_id, &csr)?;
    let crl = fs::read_to_string(ca_dir.join("ca.crl"))?;
    install_public(dir, &leaf, &ca_pem, &crl)?;

    let token = pair_proto::new_token();
    let exp = pair_proto::now_secs() + pair_proto::CODE_TTL_SECS;
    let (mut to_worker, mut from_worker, _child) = start_worker()?;
    send_line(
        &mut to_worker,
        &ToWorker::Start {
            port: opts.tunnel_port,
            token: token.clone(),
            joiner_id: joiner_id.clone(),
            exp,
        },
    )
    .await?;
    let pin = match timeout(
        Duration::from_secs(15),
        read_line::<_, FromWorker>(&mut from_worker),
    )
    .await
    {
        Ok(Ok(Some(FromWorker::Ready { pin }))) => pin,
        Ok(Ok(Some(FromWorker::Done { error: Some(e), .. }))) => bail!(e),
        _ => bail!("the pairing network worker did not start"),
    };
    let code = PairCode {
        v: 1,
        host,
        port: opts.tunnel_port,
        pin,
        token,
        tunnel_id: name.to_string(),
        direction: opts.direction.label().into(),
        listener_id: listener_id.clone(),
        joiner_id: joiner_id.clone(),
        service_port: opts.service_port,
        tunnel_port: opts.tunnel_port,
        server_name,
        exp,
    };
    setup::init(
        &req(
            dir,
            &opts.instance,
            Role::Serve,
            &listener_id,
            &joiner_id,
            &code,
        ),
        None,
    )?;
    let own_bundle = fs::read_to_string(dir.join("bundle.json"))?;
    announce(&code.encode()?);

    let mut joiner_bundle: Option<Bundle> = None;
    let mut seen_request = false;
    loop {
        match read_line::<_, FromWorker>(&mut from_worker).await? {
            None => bail!("the pairing network worker stopped"),
            Some(FromWorker::Ready { .. }) => bail!("unexpected worker message"),
            Some(FromWorker::Done { ok: true, .. }) => break,
            Some(FromWorker::Done { ok: false, error }) => {
                bail!(error.unwrap_or_else(|| "pairing failed".into()))
            }
            Some(FromWorker::Request {
                node_id,
                csr_pem,
                peer,
            }) => {
                // One request per pairing, and only for the joiner id fixed in the code.
                ensure!(
                    !seen_request && node_id == joiner_id,
                    "unexpected pairing request"
                );
                seen_request = true;
                let denied = JoinResponse {
                    ok: false,
                    reason: Some("pairing refused".into()),
                    ..Default::default()
                };
                let decision = match (
                    peer.parse::<SocketAddr>(),
                    pair_ca::csr_fingerprint(&csr_pem),
                ) {
                    (Ok(peer), Ok(verify)) => {
                        if approve(Pending {
                            node_id: node_id.clone(),
                            peer,
                            verify,
                        })
                        .await
                        {
                            match sign_join(
                                dir,
                                &ca_dir,
                                &code,
                                &node_id,
                                &csr_pem,
                                &ca_pem,
                                &own_bundle,
                            ) {
                                Ok((resp, bundle)) => {
                                    joiner_bundle = Some(bundle);
                                    resp
                                }
                                Err(_) => denied,
                            }
                        } else {
                            denied
                        }
                    }
                    _ => denied,
                };
                send_line(&mut to_worker, &ToWorker::Decision(decision)).await?;
            }
        }
    }
    let bundle = joiner_bundle.context("pairing finished without a joiner bundle")?;
    let path = dir.join("peer-bundle.json");
    fs::write(&path, serde_json::to_vec_pretty(&bundle)?)?;
    let cfg = setup::join(dir, &path, &bundle.checksum())?;
    let _ = fs::remove_file(&path);
    println!("Configuration written: {}", cfg.display());
    adopt(dir, &opts.instance)?;
    let _ = fs::remove_dir_all(dir.join("pair-join"));
    Ok(())
}

fn sign_join(
    dir: &Path,
    ca_dir: &Path,
    code: &PairCode,
    node_id: &str,
    csr_pem: &str,
    ca_pem: &str,
    own_bundle: &str,
) -> Result<(JoinResponse, Bundle)> {
    let leaf = pair_ca::sign(ca_dir, node_id, csr_pem)?;
    let work = dir.join("pair-join");
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&work)?;
    }
    let crt = work.join("joiner.crt");
    let _ = fs::remove_file(&crt);
    fs::write(&crt, &leaf)?;
    let (pin, _) = crate::tls::node_certificate_summary(&crt, node_id)?;
    let bundle = Bundle {
        version: 1,
        tunnel_id: code.tunnel_id.clone(),
        engine: "tcp-tls".into(),
        direction: code.direction.clone(),
        protocol_version: setup::default_protocol_for(EngineKind::TcpTls),
        role: Role::Forward,
        node_id: code.joiner_id.clone(),
        peer_id: code.listener_id.clone(),
        service_port: code.service_port,
        tunnel_port: code.tunnel_port,
        public: pin,
        server_name: None,
        engine_sha256: None,
    };
    let resp = JoinResponse {
        ok: true,
        certificate_pem: Some(leaf),
        ca_pem: Some(ca_pem.to_string()),
        crl_pem: Some(fs::read_to_string(ca_dir.join("ca.crl"))?),
        peer_bundle: Some(own_bundle.to_string()),
        reason: None,
    };
    Ok((resp, bundle))
}

/// Other node: decode the code, enroll through the listener, write config. `on_verify` receives the
/// short key fingerprint before the request is sent; the listener's user compares it.
pub async fn accept(
    code_text: &str,
    dir: &Path,
    instance: Option<String>,
    location: Option<Location>,
    on_verify: impl FnOnce(&str),
) -> Result<()> {
    let code = PairCode::decode(code_text)?;
    ensure_dir(dir)?;
    precheck(dir, &instance)?;
    let r = accept_inner(&code, dir, &instance, location, on_verify).await;
    if r.is_err() {
        cleanup(dir, &instance);
    }
    r
}

async fn accept_inner(
    code: &PairCode,
    dir: &Path,
    instance: &Option<String>,
    location: Option<Location>,
    on_verify: impl FnOnce(&str),
) -> Result<()> {
    let csr = gen_key_and_csr(dir, &code.joiner_id)?;
    on_verify(&pair_ca::csr_fingerprint(&csr)?);
    let (resp, session) = pair_proto::join(code, &csr).await?;
    let (Some(leaf), Some(ca), Some(crl), Some(peer), Some(session)) = (
        resp.certificate_pem,
        resp.ca_pem,
        resp.crl_pem,
        resp.peer_bundle,
        session,
    ) else {
        bail!("the other server refused the pairing (code used, expired or not approved)");
    };
    let installed = (|| -> Result<()> {
        install_public(dir, &leaf, &ca, &crl)?;
        crate::tls::verify_local_certificate(
            &dir.join("ca.crt"),
            &dir.join("ca.crl"),
            &dir.join("node.crt"),
            &format!("DNS:{}.pair.invalid", code.joiner_id),
        )?;
        setup::init(
            &req(
                dir,
                instance,
                Role::Forward,
                &code.joiner_id,
                &code.listener_id,
                code,
            ),
            None,
        )?;
        let bundle: Bundle = serde_json::from_str(&peer).context("peer bundle is invalid")?;
        ensure!(
            bundle.node_id == code.listener_id && bundle.tunnel_id == code.tunnel_id,
            "peer bundle does not match the code"
        );
        let path = dir.join("peer-bundle.json");
        fs::write(&path, &peer)?;
        let cfg = setup::join(dir, &path, &bundle.checksum())?;
        let _ = fs::remove_file(&path);
        println!("Configuration written: {}", cfg.display());
        if location == Some(Location::Iran) && code.direction == "direct" {
            set_rotation(&cfg, IRAN_ROTATE_SECS)?;
            println!(
                "Session rotation set to {IRAN_ROTATE_SECS} s (this server is in Iran and dials out)."
            );
        }
        adopt(dir, instance)
    })();
    // The listener commits only after this acknowledgement.
    let _ = session.ack(installed.is_ok()).await;
    installed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pair-flow-{tag}-{:x}", rand::random::<u64>()))
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[tokio::test]
    async fn offer_and_accept_produce_matching_working_configs() {
        // As root the worker needs the service user, which exists only on installed hosts.
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (da, db) = (tmp("a"), tmp("b"));
        let port = free_port();
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        let offer_task = tokio::spawn({
            let da = da.clone();
            async move {
                offer(
                    OfferOptions {
                        dir: da,
                        instance: None,
                        name: "Demo Tunnel".into(),
                        service_port: 9000,
                        tunnel_port: port,
                        host: Some("127.0.0.1".into()),
                        direction: Direction::Direct,
                    },
                    |_p| async { true },
                    |code| {
                        let _ = tx.send(code.to_string());
                    },
                )
                .await
            }
        });
        let code = rx.await.unwrap();
        assert!(code.starts_with("BAFT1-"));
        let shown = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let s2 = shown.clone();
        accept(&code, &db, None, None, move |v| {
            *s2.lock().unwrap() = v.to_string()
        })
        .await
        .unwrap();
        assert_eq!(shown.lock().unwrap().len(), 14, "verification string shown");
        offer_task.await.unwrap().unwrap();
        let ca: serde_json::Value =
            serde_json::from_slice(&fs::read(da.join("config.json")).unwrap()).unwrap();
        let cb: serde_json::Value =
            serde_json::from_slice(&fs::read(db.join("config.json")).unwrap()).unwrap();
        assert_eq!(ca["mode"], "serve");
        assert_eq!(cb["mode"], "forward");
        assert_eq!(cb["remote"], format!("127.0.0.1:{port}"));
        // each side pins the other's real leaf and its own CA never left its host
        assert!(ca["peer_fingerprint"].as_str().unwrap().len() == 64);
        assert!(cb["peer_fingerprint"].as_str().unwrap().len() == 64);
        assert!(da.join("pair-ca/ca.key").exists());
        assert!(!db.join("pair-ca").exists() && !db.join("ca.key").exists());
        let _ = fs::remove_dir_all(&da);
        let _ = fs::remove_dir_all(&db);
    }

    #[tokio::test]
    async fn unapproved_join_leaves_no_files_on_the_joiner() {
        // As root the worker needs the service user, which exists only on installed hosts.
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (da, db) = (tmp("na"), tmp("nb"));
        let port = free_port();
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        let t = tokio::spawn({
            let da = da.clone();
            async move {
                let _ = offer(
                    OfferOptions {
                        dir: da,
                        instance: None,
                        name: "x".into(),
                        service_port: 9000,
                        tunnel_port: port,
                        host: Some("127.0.0.1".into()),
                        direction: Direction::Direct,
                    },
                    |_p| async { false },
                    |code| {
                        let _ = tx.send(code.to_string());
                    },
                )
                .await;
            }
        });
        let code = rx.await.unwrap();
        assert!(accept(&code, &db, None, None, |_| {}).await.is_err());
        for f in ["node.key", "node.crt", "config.json"] {
            assert!(!db.join(f).exists(), "{f}");
        }
        t.abort();
        let _ = fs::remove_dir_all(&da);
        let _ = fs::remove_dir_all(&db);
    }

    #[tokio::test]
    async fn an_existing_configuration_is_never_overwritten() {
        let d = tmp("pre");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("config.json"), b"{}").unwrap();
        let err = offer(
            OfferOptions {
                dir: d.clone(),
                instance: None,
                name: "x".into(),
                service_port: 9000,
                tunnel_port: 18443,
                host: Some("127.0.0.1".into()),
                direction: Direction::Direct,
            },
            |_p| async { true },
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(format!("{err:#}").contains("already holds"));
        assert_eq!(fs::read(d.join("config.json")).unwrap(), b"{}", "untouched");
        let _ = fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn an_iran_dialer_gets_session_rotation_and_others_do_not() {
        // As root the worker needs the service user, which exists only on installed hosts.
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        for (loc, want) in [
            (Some(Location::Iran), Some(10)),
            (Some(Location::Abroad), None),
        ] {
            let (da, db) = (tmp("la"), tmp("lb"));
            let port = free_port();
            let (tx, rx) = tokio::sync::oneshot::channel::<String>();
            let t = tokio::spawn({
                let da = da.clone();
                async move {
                    offer(
                        OfferOptions {
                            dir: da,
                            instance: None,
                            name: "loc".into(),
                            service_port: 9000,
                            tunnel_port: port,
                            host: Some("127.0.0.1".into()),
                            direction: Direction::Direct,
                        },
                        |_p| async { true },
                        |code| {
                            let _ = tx.send(code.to_string());
                        },
                    )
                    .await
                }
            });
            let code = rx.await.unwrap();
            accept(&code, &db, None, loc, |_| {}).await.unwrap();
            t.await.unwrap().unwrap();
            let cb: serde_json::Value =
                serde_json::from_slice(&fs::read(db.join("config.json")).unwrap()).unwrap();
            assert_eq!(cb["session_rotate_secs"].as_u64(), want, "{loc:?}");
            let ca: serde_json::Value =
                serde_json::from_slice(&fs::read(da.join("config.json")).unwrap()).unwrap();
            assert!(
                ca.get("session_rotate_secs").is_none(),
                "listener never rotates"
            );
            let _ = fs::remove_dir_all(&da);
            let _ = fs::remove_dir_all(&db);
        }
    }

    #[tokio::test]
    async fn reverse_pairing_puts_the_ingress_on_the_listener_and_the_target_on_the_dialer() {
        // As root the worker needs the service user, which exists only on installed hosts.
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (da, db) = (tmp("ra"), tmp("rb"));
        let port = free_port();
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        let t = tokio::spawn({
            let da = da.clone();
            async move {
                offer(
                    OfferOptions {
                        dir: da,
                        instance: None,
                        name: "rev".into(),
                        service_port: 9000,
                        tunnel_port: port,
                        host: Some("127.0.0.1".into()),
                        direction: Direction::Reverse,
                    },
                    |_p| async { true },
                    |code| {
                        let _ = tx.send(code.to_string());
                    },
                )
                .await
            }
        });
        let code = rx.await.unwrap();
        assert_eq!(PairCode::decode(&code).unwrap().direction, "reverse");
        // an Iran answer must not add rotation, which Reverse cannot use
        accept(&code, &db, None, Some(Location::Iran), |_| {})
            .await
            .unwrap();
        t.await.unwrap().unwrap();
        let read = |d: &Path| -> serde_json::Value {
            serde_json::from_slice(&fs::read(d.join("config.json")).unwrap()).unwrap()
        };
        let (ca, cb) = (read(&da), read(&db));
        assert_eq!(ca["mode"], "serve");
        assert_eq!(ca["ingress"], "127.0.0.1:9000");
        assert!(ca.get("target").is_none());
        assert_eq!(cb["mode"], "forward");
        assert_eq!(cb["target"], "127.0.0.1:9000");
        assert!(cb.get("listen").is_none());
        assert!(cb.get("session_rotate_secs").is_none());
        assert_eq!(cb["remote"], format!("127.0.0.1:{port}"));
        let _ = fs::remove_dir_all(&da);
        let _ = fs::remove_dir_all(&db);
    }
}
