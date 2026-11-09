//! Local status API (P3-002 slice A1; P1 operations evidence, ADR-0024).
//!
//! Loopback only, bearer-token protected. It reports the redacted support report, the tunnel's
//! end-to-end evidence tiers, traffic counters and peer-aware diagnostics read from the running
//! unit. It never starts, stops or changes a tunnel except through the confirmed Apply stages.

use std::{
    io::Write,
    net::SocketAddr,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use rand::RngCore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Mutex, Semaphore},
    time::timeout,
};

use crate::config::RuntimeConfig;

const MIN_TOKEN_LEN: usize = 32;
const MAX_REQUEST_BYTES: usize = 8 * 1024;
const MAX_BODY_BYTES: usize = 64 * 1024;
const DESIRED_FILE: &str = "desired.json";
const AUDIT_FILE: &str = "audit.log";
const DIAGNOSTICS_FILE: &str = "diagnostics.json";
/// Agent-side traffic samples kept in memory (one hour at the default interval).
const TRAFFIC_LEN: usize = 720;
/// Bidirectional payload within this window counts as "data path observed".
const DATA_WINDOW_S: u64 = 60;
const MAX_CONNECTIONS: usize = 16;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Whole-request bound. Must exceed the Apply `start` wait (20 s) plus settle.
const HANDLER_TIMEOUT: Duration = Duration::from_secs(45);
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const PEER_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Create a random API token (hex, 0600, never overwrites). Returns nothing secret.
pub fn write_token(path: &Path) -> Result<()> {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| {
            format!(
                "cannot create token file {} (never overwritten)",
                path.display()
            )
        })?;
    file.write_all(hex::encode(bytes).as_bytes())?;
    Ok(())
}

pub fn read_token(path: &Path) -> Result<String> {
    let meta = std::fs::symlink_metadata(path)
        .with_context(|| format!("cannot read token file {}", path.display()))?;
    if !meta.file_type().is_file() {
        bail!("token file must be a regular file");
    }
    if meta.permissions().mode() & 0o077 != 0 {
        bail!("token file must not be accessible by group or others (use mode 0600)");
    }
    let token = std::fs::read_to_string(path)?.trim().to_string();
    if token.len() < MIN_TOKEN_LEN {
        bail!("token must be at least {MIN_TOKEN_LEN} characters");
    }
    Ok(token)
}

pub fn parse_listen(value: &str) -> Result<SocketAddr> {
    let addr: SocketAddr = value.parse().context("agent listen must be IP:port")?;
    if !addr.ip().is_loopback() {
        bail!("the agent API only listens on loopback addresses");
    }
    if addr.port() == 0 {
        bail!("agent listen port must not be 0");
    }
    Ok(addr)
}

pub struct Ctx {
    pub config: RuntimeConfig,
    pub token: String,
    /// Directory for `desired.json` and `audit.log`; `None` disables the desired-state routes.
    pub state_dir: Option<PathBuf>,
    pub lock: Mutex<()>,
    /// Listener probes, oldest first; kept in memory only (one hour at the default interval).
    history: std::sync::Mutex<std::collections::VecDeque<Sample>>,
    /// `baft-tunnel@NAME` this agent reports on; `None` leaves unit state Unknown.
    instance: Option<String>,
    /// The unit's runtime directory (session, counters, last failure); `None` = no unit evidence.
    runtime_dir: Option<PathBuf>,
    /// Unit counter readings, oldest first, deduplicated by sample time.
    traffic: std::sync::Mutex<std::collections::VecDeque<crate::observe::TrafficSample>>,
}

const SAMPLE_SECONDS: u64 = 5;
const HISTORY_LEN: usize = 720;

#[derive(Clone, Copy)]
struct Sample {
    ts: u64,
    up: bool,
    rtt_ms: Option<f64>,
}

impl Ctx {
    pub fn new(config: RuntimeConfig, token: String, state_dir: Option<PathBuf>) -> Self {
        Self {
            config,
            token,
            state_dir,
            lock: Mutex::new(()),
            history: std::sync::Mutex::new(std::collections::VecDeque::new()),
            instance: None,
            runtime_dir: None,
            traffic: std::sync::Mutex::new(std::collections::VecDeque::new()),
        }
    }

    /// Report on `baft-tunnel@NAME`, reading its evidence from `runtime_dir`.
    pub fn with_unit(mut self, instance: Option<String>, runtime_dir: Option<PathBuf>) -> Self {
        self.instance = instance;
        self.runtime_dir = runtime_dir;
        self
    }

    /// Non-secret actor id for audit lines: a domain-separated hash prefix of the API token.
    fn actor(&self) -> Value {
        let mut h = Sha256::new();
        h.update(b"baft-tunnel/agent-token/v1\0");
        h.update(self.token.as_bytes());
        json!({"kind": "api_token", "fingerprint": hex::encode(&h.finalize()[..8])})
    }

    fn record_traffic(&self, sample: crate::observe::TrafficSample) {
        let mut list = self.traffic.lock().unwrap_or_else(|e| e.into_inner());
        if list
            .back()
            .is_some_and(|last| last.t == sample.t && last.pid == sample.pid)
        {
            return;
        }
        if list.len() >= TRAFFIC_LEN {
            list.pop_front();
        }
        list.push_back(sample);
    }

    fn traffic_samples(&self) -> Vec<crate::observe::TrafficSample> {
        let list = self.traffic.lock().unwrap_or_else(|e| e.into_inner());
        list.iter().copied().collect()
    }

    fn evidence(&self, main_pid: Option<u32>) -> crate::observe::Evidence {
        self.runtime_dir
            .as_deref()
            .map(|d| crate::observe::read(d, main_pid))
            .unwrap_or_default()
    }

    fn record(&self, sample: Sample) {
        let mut history = self.history.lock().unwrap_or_else(|e| e.into_inner());
        if history.len() >= HISTORY_LEN {
            history.pop_front();
        }
        history.push_back(sample);
    }

    async fn probe(&self) -> Sample {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let start = std::time::Instant::now();
        let up = match self.config.listen_addr().parse::<SocketAddr>() {
            Ok(addr) => matches!(
                timeout(PROBE_TIMEOUT, TcpStream::connect(addr)).await,
                Ok(Ok(_))
            ),
            Err(_) => false,
        };
        Sample {
            ts,
            up,
            rtt_ms: up.then(|| start.elapsed().as_secs_f64() * 1000.0),
        }
    }
}

async fn sampler(ctx: Arc<Ctx>) {
    let mut tick = tokio::time::interval(Duration::from_secs(SAMPLE_SECONDS));
    loop {
        tick.tick().await;
        let sample = ctx.probe().await;
        ctx.record(sample);
        if let Some(t) = ctx
            .evidence(None)
            .counters
            .as_ref()
            .and_then(crate::observe::sample)
        {
            ctx.record_traffic(t);
        }
    }
}

pub async fn run(
    config: RuntimeConfig,
    listen: SocketAddr,
    token: String,
    state_dir: Option<PathBuf>,
    instance: Option<String>,
) -> Result<()> {
    if let Some(name) = &instance {
        if !crate::apply::valid_instance(name) {
            bail!("invalid --instance name");
        }
    }
    let runtime_dir = instance
        .as_ref()
        .map(|name| Path::new("/run/baft-tunnel").join(name));
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("cannot bind agent API on {listen}"))?;
    eprintln!("Agent API listening on {listen} (token required; never applies a config itself)");
    let ctx = Arc::new(Ctx::new(config, token, state_dir).with_unit(instance, runtime_dir));
    tokio::spawn(sampler(ctx.clone()));
    serve(listener, ctx).await
}

pub async fn serve(listener: TcpListener, ctx: Arc<Ctx>) -> Result<()> {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (stream, _) = listener.accept().await?;
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let _ = timeout(HANDLER_TIMEOUT, handle(stream, &ctx)).await;
            drop(permit);
        });
    }
}

async fn handle(mut stream: TcpStream, ctx: &Ctx) -> Result<()> {
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() >= MAX_REQUEST_BYTES {
            return respond(&mut stream, 431, &json!({"error": "request too large"})).await;
        }
        let n = timeout(IO_TIMEOUT, stream.read(&mut chunk)).await??;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let length = header(&head, "content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if length > MAX_BODY_BYTES {
        return respond(&mut stream, 413, &json!({"error": "body too large"})).await;
    }
    while buf.len() < head_end + length {
        let n = timeout(IO_TIMEOUT, stream.read(&mut chunk)).await??;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = &buf[head_end..head_end + length];
    if let Some((kind, text)) = static_asset(&head) {
        return respond_static(&mut stream, kind, text).await;
    }
    let (status, out) = route(&head, body, ctx).await;
    respond(&mut stream, status, &out).await
}

/// Fuzz entry (see fuzz/): the request parser and router with no state directory.
#[allow(dead_code)] // used by the fuzz harness only
pub fn fuzz_route(head: &str, body: &[u8]) -> (u16, Value) {
    let config = serde_json::from_value(json!({
        "mode": "forward", "listen": "127.0.0.1:1", "remote": "192.0.2.1:8443",
        "server_name": "x", "local_id": "a", "peer_id": "b", "tunnel_id": "t",
        "direction": "direct", "peer_fingerprint": "00", "key": "/nonexistent/key.pem"
    }))
    .expect("fixed config");
    let ctx = Ctx::new(config, "t".repeat(40), None);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let _ = static_asset(head);
    runtime.block_on(route(head, body, &ctx))
}

/// The panel is a fixed set of files compiled into the binary; they hold no secrets and no data.
const PANEL_ASSETS: &[(&str, &str, &str)] = &[
    (
        "/index.html",
        "text/html; charset=utf-8",
        include_str!("../panel/index.html"),
    ),
    (
        "/panel.css",
        "text/css; charset=utf-8",
        include_str!("../panel/panel.css"),
    ),
    (
        "/i18n.js",
        "text/javascript; charset=utf-8",
        include_str!("../panel/i18n.js"),
    ),
    (
        "/charts.js",
        "text/javascript; charset=utf-8",
        include_str!("../panel/charts.js"),
    ),
    (
        "/app.js",
        "text/javascript; charset=utf-8",
        include_str!("../panel/app.js"),
    ),
    (
        "/pages.js",
        "text/javascript; charset=utf-8",
        include_str!("../panel/pages.js"),
    ),
    (
        "/boot.js",
        "text/javascript; charset=utf-8",
        include_str!("../panel/boot.js"),
    ),
];

fn static_asset(head: &str) -> Option<(&'static str, &'static str)> {
    let mut parts = head.split("\r\n").next()?.split(' ');
    if parts.next()? != "GET" {
        return None;
    }
    let path = match parts.next()? {
        "/" => "/index.html",
        other => other,
    };
    PANEL_ASSETS
        .iter()
        .find(|(name, _, _)| *name == path)
        .map(|(_, kind, text)| (*kind, *text))
}

async fn respond_static(stream: &mut TcpStream, kind: &str, text: &str) -> Result<()> {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        text.len()
    );
    timeout(IO_TIMEOUT, async {
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(text.as_bytes()).await?;
        stream.shutdown().await
    })
    .await??;
    Ok(())
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.split("\r\n")
        .skip(1)
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim())
}

async fn route(head: &str, body: &[u8], ctx: &Ctx) -> (u16, Value) {
    let mut parts = head.split("\r\n").next().unwrap_or_default().split(' ');
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if path == "/v1/health" {
        return if method == "GET" {
            (200, json!({"ok": true}))
        } else {
            (405, json!({"error": "method not allowed"}))
        };
    }
    if !matches!(
        path,
        "/v1/status"
            | "/v1/desired"
            | "/v1/setup/plan"
            | "/v1/setup/preflight"
            | "/v1/setup/apply"
            | "/v1/history"
            | "/v1/security"
            | "/v1/audit"
            | "/v1/engines"
            | "/v1/traffic"
            | "/v1/diagnostics"
    ) {
        return (404, json!({"error": "not found"}));
    }
    let presented = header(head, "authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !constant_time_eq(presented.as_bytes(), ctx.token.as_bytes()) {
        return (401, json!({"error": "unauthorized"}));
    }
    match (method, path) {
        ("GET", "/v1/status") => (200, status(ctx).await),
        ("GET", "/v1/traffic") => (200, traffic(ctx)),
        ("GET", "/v1/diagnostics") => get_diagnostics(ctx),
        ("POST", "/v1/diagnostics") => (200, run_diagnostics(ctx).await),
        ("GET", "/v1/history") => (200, history(ctx)),
        ("GET", "/v1/security") => (200, security(&ctx.config)),
        ("GET", "/v1/audit") => get_audit(ctx),
        ("GET", "/v1/engines") => (200, engines()),
        ("GET", "/v1/desired") => get_desired(ctx),
        ("PUT", "/v1/desired") => put_desired(ctx, body).await,
        ("POST", "/v1/setup/plan") => setup_plan(body),
        ("POST", "/v1/setup/preflight") => preflight(body).await,
        ("POST", "/v1/setup/apply") => setup_apply(ctx, body).await,
        _ => (405, json!({"error": "method not allowed"})),
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanBody {
    engine: crate::config::EngineKind,
    role: crate::setup::Role,
    direction: crate::authorization::ConnectionDirection,
    node_id: String,
    peer_id: String,
    tunnel_id: String,
    service_port: u16,
    tunnel_port: Option<u16>,
    peer_host: Option<String>,
    server_name: Option<String>,
    dir: Option<PathBuf>,
    instance: Option<String>,
}

impl PlanBody {
    fn to_request(&self) -> crate::setup::PlanRequest {
        crate::setup::PlanRequest {
            engine: self.engine,
            role: self.role,
            direction: self.direction,
            node_id: self.node_id.clone(),
            peer_id: self.peer_id.clone(),
            tunnel_id: self.tunnel_id.clone(),
            service_port: self.service_port,
            tunnel_port: self
                .tunnel_port
                .unwrap_or(crate::setup::DEFAULT_TUNNEL_PORT),
            peer_host: self.peer_host.clone(),
            server_name: self.server_name.clone(),
            dir: self
                .dir
                .clone()
                .unwrap_or_else(|| PathBuf::from("/etc/baft-tunnel")),
            instance: self.instance.clone(),
        }
    }
}

fn check(id: &str, result: &str, evidence: String) -> Value {
    json!({"id": id, "result": result, "evidence": evidence})
}

async fn port_accepting(port: u16) -> bool {
    matches!(
        timeout(
            PROBE_TIMEOUT,
            TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port)))
        )
        .await,
        Ok(Ok(_))
    )
}

/// Read-only preflight of THIS host for a planned tunnel. Results are pass / warning / blocked /
/// unknown; anything the agent cannot observe from here is `unknown`, never `pass`. Writes
/// nothing, runs nothing, and does not contact the peer.
async fn preflight(body: &[u8]) -> (u16, Value) {
    let request: PlanBody = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(error) => {
            return (
                422,
                json!({"error": "invalid request", "detail": error.to_string()}),
            );
        }
    };
    let req = request.to_request();
    let mut checks: Vec<Value> = Vec::new();
    match crate::setup::plan(&req) {
        Ok(_) => checks.push(check(
            "planner",
            "pass",
            "the same planner as the command line accepted these settings".into(),
        )),
        Err(error) => {
            checks.push(check("planner", "blocked", format!("{error:#}")));
            return (200, preflight_report(checks));
        }
    }
    {
        let direction = serde_json::to_value(req.direction)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        if crate::capabilities::supports_direction(req.engine.as_str(), &direction) {
            checks.push(check(
                "direction_support",
                "pass",
                format!(
                    "{} supports the {direction} direction in this build",
                    req.engine.as_str()
                ),
            ));
        } else {
            checks.push(check(
                "direction_support",
                "blocked",
                format!("the {direction} direction is not operational for {} in this build (no real data path has been proven for it)", req.engine.as_str()),
            ));
        }
    }
    let dir = &req.dir;
    if dir.is_dir() {
        checks.push(check(
            "directory",
            "pass",
            "the setup directory exists on this host".into(),
        ));
    } else if dir.exists() {
        checks.push(check(
            "directory",
            "blocked",
            "the setup path exists but is not a directory".into(),
        ));
    } else {
        checks.push(check(
            "directory",
            "warning",
            "the setup directory does not exist on this host yet".into(),
        ));
    }
    if dir.join("pairing-state.json").exists() {
        checks.push(check(
            "existing_pairing",
            "blocked",
            "this directory already holds a pairing; setup never overwrites it. Choose another directory or instance".into(),
        ));
    } else {
        checks.push(check(
            "existing_pairing",
            "pass",
            "no earlier pairing in this directory".into(),
        ));
    }
    if req.engine == crate::config::EngineKind::TcpTls {
        let missing: Vec<&str> = ["node.key", "node.crt", "ca.crt", "ca.crl"]
            .into_iter()
            .filter(|n| !dir.join(n).is_file())
            .collect();
        if missing.is_empty() {
            checks.push(check("pki_files", "pass", "certificate, key, CA and CRL files are present (validity is checked by setup init)".into()));
        } else {
            checks.push(check(
                "pki_files",
                "blocked",
                format!("missing: {}", missing.join(", ")),
            ));
        }
    } else if dir.join("node.noise").exists() {
        checks.push(check(
            "node_key",
            "blocked",
            "a node key already exists here; setup never overwrites it".into(),
        ));
    } else {
        checks.push(check(
            "node_key",
            "pass",
            "no node key yet; setup init will create one".into(),
        ));
    }
    let serve = req.role == crate::setup::Role::Serve;
    let service_up = port_accepting(req.service_port).await;
    match (serve, service_up) {
        (true, true) => checks.push(check("service_port", "pass", format!("something accepts connections on 127.0.0.1:{}", req.service_port))),
        (true, false) => checks.push(check("service_port", "warning", format!("nothing accepts connections on 127.0.0.1:{} yet; start the service before users connect", req.service_port))),
        (false, true) => checks.push(check("service_port", "warning", format!("127.0.0.1:{} is already in use on this host", req.service_port))),
        (false, false) => checks.push(check("service_port", "pass", format!("127.0.0.1:{} is free", req.service_port))),
    }
    if serve {
        if port_accepting(req.tunnel_port).await {
            checks.push(check(
                "tunnel_port",
                "warning",
                format!(
                    "port {} already accepts connections here (possibly a running tunnel)",
                    req.tunnel_port
                ),
            ));
        } else {
            checks.push(check(
                "tunnel_port",
                "pass",
                format!("port {} is not in use on loopback", req.tunnel_port),
            ));
        }
    }
    if req.instance.is_some() {
        let unit = [
            "/etc/systemd/system/baft-tunnel@.service",
            "/lib/systemd/system/baft-tunnel@.service",
            "/usr/lib/systemd/system/baft-tunnel@.service",
        ];
        if unit.iter().any(|u| std::path::Path::new(u).is_file()) {
            checks.push(check(
                "service_unit",
                "pass",
                "the baft-tunnel@ systemd unit is installed".into(),
            ));
        } else {
            checks.push(check("service_unit", "warning", "the baft-tunnel@ systemd unit is not installed on this host; run scripts/install.sh".into()));
        }
    }
    match (req.role, req.peer_host.as_deref()) {
        (crate::setup::Role::Forward, Some(host)) => {
            checks.push(probe_peer(host, req.tunnel_port).await);
        }
        _ => checks.push(check(
            "peer_reachability",
            "unknown",
            "this side listens, so it cannot test the peer from here; not verified".into(),
        )),
    }
    for (id, why) in [
        (
            "peer_identity",
            "the peer's identity can only be verified during pairing; not verified",
        ),
        (
            "firewall",
            "the agent does not inspect firewall rules; not verified",
        ),
    ] {
        checks.push(check(id, "unknown", why.into()));
    }
    (200, preflight_report(checks))
}

/// One outbound TCP connect to the peer's tunnel port (no data is sent). It proves only that
/// something answers; it says nothing about who answers.
async fn probe_peer(host: &str, port: u16) -> Value {
    let Ok(ip) = host.parse::<std::net::IpAddr>() else {
        return check(
            "peer_reachability",
            "unknown",
            "the peer address is not an IP address; not probed".into(),
        );
    };
    match timeout(
        PEER_PROBE_TIMEOUT,
        TcpStream::connect(SocketAddr::new(ip, port)),
    )
    .await
    {
        Ok(Ok(_)) => check(
            "peer_reachability",
            "pass",
            format!(
                "a TCP connection to the peer on port {port} was accepted (who answered is not verified)"
            ),
        ),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => check(
            "peer_reachability",
            "warning",
            format!("the peer host answered but nothing listens on port {port} yet"),
        ),
        Ok(Err(e)) => check(
            "peer_reachability",
            "blocked",
            format!("cannot reach the peer on port {port}: {e}"),
        ),
        Err(_) => check(
            "peer_reachability",
            "blocked",
            format!(
                "no answer from the peer on port {port} within {} s",
                PEER_PROBE_TIMEOUT.as_secs()
            ),
        ),
    }
}

fn remediation(id: &str) -> &'static str {
    match id {
        "planner" => "Fix the highlighted setting; the same rule applies on the command line.",
        "direction_support" => {
            "Choose the Direct direction, or tcp-tls for the Reverse preview; Reverse is not available for this engine yet."
        }
        "directory" => "Create the setup directory or choose another one.",
        "existing_pairing" => {
            "Choose another directory or instance; setup never overwrites a pairing."
        }
        "pki_files" => "Provide the certificate, key, CA and CRL files in the setup directory.",
        "node_key" => "Choose another directory; setup never overwrites a node key.",
        "service_port" => "Start the local service, or pick a free port.",
        "tunnel_port" => "Check whether a tunnel already runs on this port, or pick another port.",
        "service_unit" => "Run scripts/install.sh to install the systemd unit.",
        "peer_reachability" => {
            "Start the peer first, check its address and port, and allow the port in its firewall."
        }
        "peer_identity" => {
            "Identity is verified when pairing; compare the fingerprints on both servers."
        }
        "firewall" => "Check the firewall on both servers by hand; the agent cannot inspect it.",
        _ => "",
    }
}

fn check_meta(id: &str, result: &str) -> (&'static str, &'static str) {
    let scope = if matches!(id, "peer_reachability" | "peer_identity" | "firewall") {
        "peer"
    } else {
        "local"
    };
    let source = match (id, result) {
        (_, "unknown") => "not-observed",
        ("peer_reachability", _) => "network-probe",
        _ => "local-observation",
    };
    (scope, source)
}

fn preflight_report(checks: Vec<Value>) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let checks: Vec<Value> = checks
        .into_iter()
        .map(|mut c| {
            let id = c["id"].as_str().unwrap_or("").to_string();
            let result = c["result"].as_str().unwrap_or("").to_string();
            let (scope, source) = check_meta(&id, &result);
            c["code"] = json!(format!("pf.{id}.{result}"));
            c["scope"] = json!(scope);
            c["source"] = json!(source);
            c["checked_at"] = json!(now);
            if result != "pass" {
                c["remediation"] = json!(remediation(&id));
            }
            c
        })
        .collect();
    let has = |r: &str| checks.iter().any(|c| c["result"] == r);
    let overall = if has("blocked") {
        "blocked"
    } else if has("unknown") {
        "unknown"
    } else if has("warning") {
        "warning"
    } else {
        "pass"
    };
    json!({"overall": overall, "checks": checks, "applied": false, "apply_available": true})
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyBody {
    stage: crate::apply::Stage,
    /// Must be literally `true`: Apply writes files on this host.
    confirm: bool,
    idempotency_key: String,
    plan: serde_json::Value,
    bundle: Option<String>,
    checksum: Option<String>,
}

/// Authorized, gated Apply -> Verify -> Receipt for the two local setup stages (ADR-0022).
async fn setup_apply(ctx: &Ctx, body: &[u8]) -> (u16, Value) {
    let Some(state_dir) = &ctx.state_dir else {
        return (
            404,
            json!({"error": "desired state is disabled; receipts need the state directory"}),
        );
    };
    let req: ApplyBody = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(error) => {
            return (
                422,
                json!({"error": "invalid request", "detail": error.to_string()}),
            );
        }
    };
    if !req.confirm {
        return (422, json!({"error": "apply requires \"confirm\": true"}));
    }
    if !crate::apply::valid_key(&req.idempotency_key) {
        return (
            422,
            json!({"error": "idempotency_key must be 8-64 characters of A-Z a-z 0-9 - _"}),
        );
    }
    let plan_bytes = req.plan.to_string().into_bytes();
    let plan: PlanBody = match serde_json::from_slice(&plan_bytes) {
        Ok(p) => p,
        Err(error) => {
            return (
                422,
                json!({"error": "invalid plan", "detail": error.to_string()}),
            );
        }
    };
    let request = crate::apply::ApplyRequest {
        stage: req.stage,
        plan: plan.to_request(),
        key: req.idempotency_key,
        bundle: req.bundle,
        checksum: req.checksum,
    };
    // One apply at a time per agent.
    let _guard = ctx.lock.lock().await;
    let gate = if request.stage == crate::apply::Stage::Init {
        let (code, report) = preflight(&plan_bytes).await;
        if code != 200 {
            return (code, report);
        }
        let list = report["checks"].as_array().cloned().unwrap_or_default();
        // Only checks about this host gate the local init; peer reachability is reported, not required.
        let blocked = list
            .iter()
            .filter(|c| c["result"] == "blocked" && c["scope"] != "peer")
            .filter_map(|c| c["id"].as_str().map(str::to_string))
            .collect();
        Some(crate::apply::Gate {
            overall: report["overall"].as_str().unwrap_or("unknown").to_string(),
            checks: list
                .iter()
                .map(|c| {
                    (
                        c["id"].as_str().unwrap_or("").to_string(),
                        c["result"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect(),
            blocked,
        })
    } else {
        None
    };
    let receipts_dir = state_dir.join("receipts");
    // Start waits for systemd; keep the async runtime free while it does.
    let (code, receipt) = match tokio::task::spawn_blocking({
        let request = request.clone();
        move || crate::apply::run(&request, &receipts_dir, gate.as_ref())
    })
    .await
    {
        Ok(out) => out,
        Err(e) => return (500, json!({"error": format!("apply task failed: {e}")})),
    };
    let digest = receipt["request_sha256"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let result = receipt["result"].as_str().unwrap_or("conflict");
    if receipt["replayed"] != json!(true) {
        let stage = match request.stage {
            crate::apply::Stage::Init => "init",
            crate::apply::Stage::Join => "join",
            crate::apply::Stage::Start => "start",
        };
        let _ = audit(
            state_dir,
            ctx,
            AuditEvent {
                action: "apply",
                result,
                resource: format!("setup/{stage}"),
                node: Some(&request.plan.node_id),
                instance: request.plan.instance.as_deref(),
                sha256: &digest,
                detail: stage,
                receipt_id: receipt["id"].as_str(),
            },
        );
    }
    (code, receipt)
}

fn shell_quote(value: &str) -> Result<String> {
    if value.contains('\'') || value.chars().any(char::is_control) {
        bail!("value contains characters that cannot be quoted safely");
    }
    Ok(format!("'{value}'"))
}

/// Wizard backend: the same validated `setup::plan` as the CLI. Writes nothing and runs nothing;
/// it returns the exact commands the operator then runs on the host.
fn setup_plan(body: &[u8]) -> (u16, Value) {
    let request: PlanBody = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(error) => {
            return (
                422,
                json!({"error": "invalid request", "detail": error.to_string()}),
            );
        }
    };
    let result = (|| -> Result<Value> {
        let req = request.to_request();
        let plan = crate::setup::plan(&req)?;
        let dir = shell_quote(&req.dir.to_string_lossy())?;
        let enum_arg = |v: &Value| v.as_str().unwrap_or_default().to_string();
        let engine = req.engine.as_str().to_string();
        let role = enum_arg(&serde_json::to_value(req.role)?);
        let direction = enum_arg(&serde_json::to_value(req.direction)?);
        let mut init = format!(
            "baft-tunnel setup init --engine {engine} --role {role} --direction {direction} --node-id {} --peer-id {} --tunnel-id {} --service-port {} --tunnel-port {} --dir {dir}",
            shell_quote(&req.node_id)?,
            shell_quote(&req.peer_id)?,
            shell_quote(&req.tunnel_id)?,
            req.service_port,
            req.tunnel_port
        );
        if let Some(h) = &req.peer_host {
            init += &format!(" --peer-host {}", shell_quote(h)?);
        }
        if let Some(n) = &req.server_name {
            init += &format!(" --server-name {}", shell_quote(n)?);
        }
        if let Some(name) = &req.instance {
            init += &format!(" --instance {}", shell_quote(name)?);
        }
        let join = format!(
            "baft-tunnel setup join --dir {dir} --bundle <peer bundle.json> --checksum <peer checksum>"
        );
        let commands: Vec<String> = match &req.instance {
            // Installed service: the unit runs as the baft-tunnel user, so it must own its key.
            Some(name) => {
                let key = if req.engine == crate::config::EngineKind::TcpTls {
                    "node.key"
                } else {
                    "node.noise"
                };
                let d = req.dir.to_string_lossy();
                vec![
                    init,
                    join,
                    format!(
                        "chown baft-tunnel:baft-tunnel {}",
                        shell_quote(&format!("{d}/{key}"))?
                    ),
                    // setup writes NAME.json as root:root 0640; the unit reads it as baft-tunnel.
                    format!(
                        "chgrp baft-tunnel {}",
                        shell_quote(&format!("{d}/{name}.json"))?
                    ),
                    format!("systemctl enable --now baft-tunnel@{name}"),
                ]
            }
            None => vec![
                init,
                join,
                format!(
                    "baft-tunnel apply --config {}/config.json --confirm",
                    req.dir.to_string_lossy()
                ),
            ],
        };
        let files: Vec<Value> = plan
            .files
            .iter()
            .map(|f| json!({"path": f.path.to_string_lossy(), "mode": f.mode, "note": f.note}))
            .collect();
        Ok(
            json!({"config": plan.config, "files": files, "pending": plan.pending, "commands": commands, "applied": false}),
        )
    })();
    match result {
        Ok(v) => (200, v),
        Err(error) => (
            422,
            json!({"error": "plan rejected", "detail": format!("{error:#}")}),
        ),
    }
}

fn history(ctx: &Ctx) -> Value {
    let history = ctx.history.lock().unwrap_or_else(|e| e.into_inner());
    let samples: Vec<Value> = history
        .iter()
        .map(|s| json!({"t": s.ts, "up": s.up, "rtt_ms": s.rtt_ms}))
        .collect();
    json!({"interval_s": SAMPLE_SECONDS, "capacity": HISTORY_LEN, "samples": samples})
}

/// Certificate and CRL lifetime, when the engine uses them. Values are seconds (negative = expired).
fn security(config: &RuntimeConfig) -> Value {
    let Some((crl, cert)) = config.tls_files() else {
        return json!({"engine": config.engine_name(), "tls": false});
    };
    json!({
        "engine": config.engine_name(),
        "tls": true,
        "crl_seconds_remaining": crate::tls::crl_seconds_remaining(crl).ok(),
        "certificate_seconds_remaining": crate::tls::certificate_seconds_remaining(cert).ok(),
    })
}

fn get_audit(ctx: &Ctx) -> (u16, Value) {
    let Some(dir) = &ctx.state_dir else {
        return (404, json!({"error": "desired state is disabled"}));
    };
    let text = std::fs::read_to_string(dir.join(AUDIT_FILE)).unwrap_or_default();
    let mut entries: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let start = entries.len().saturating_sub(100);
    (200, json!({"entries": entries.split_off(start)}))
}

/// Engine capability contract (see `capabilities`): real, versioned, unknown stays unknown.
fn engines() -> Value {
    crate::capabilities::catalog()
}

fn get_desired(ctx: &Ctx) -> (u16, Value) {
    let Some(dir) = &ctx.state_dir else {
        return (404, json!({"error": "desired state is disabled"}));
    };
    match std::fs::read(dir.join(DESIRED_FILE)) {
        Ok(bytes) => match serde_json::from_slice::<RuntimeConfig>(&bytes) {
            Ok(cfg) => (
                200,
                json!({"sha256": hex::encode(Sha256::digest(&bytes)), "config": cfg.support_report(), "applied": false}),
            ),
            Err(_) => (500, json!({"error": "stored desired state is unreadable"})),
        },
        Err(_) => (404, json!({"error": "no desired state stored"})),
    }
}

async fn put_desired(ctx: &Ctx, body: &[u8]) -> (u16, Value) {
    let Some(dir) = &ctx.state_dir else {
        return (404, json!({"error": "desired state is disabled"}));
    };
    let _guard = ctx.lock.lock().await;
    let digest = hex::encode(Sha256::digest(body));
    let checked = serde_json::from_slice::<RuntimeConfig>(body)
        .map_err(anyhow::Error::from)
        .and_then(|c| c.validate());
    if let Err(error) = checked {
        let detail = format!("{error:#}");
        let _ = audit(
            dir,
            ctx,
            AuditEvent {
                action: "put_desired",
                result: "rejected",
                resource: "desired_state".into(),
                node: None,
                instance: None,
                sha256: &digest,
                detail: &detail,
                receipt_id: None,
            },
        );
        return (
            422,
            json!({"error": "configuration rejected", "detail": detail}),
        );
    }
    let accepted = AuditEvent {
        action: "put_desired",
        result: "accepted",
        resource: "desired_state".into(),
        node: None,
        instance: None,
        sha256: &digest,
        detail: "",
        receipt_id: None,
    };
    match store(dir, body).and_then(|()| audit(dir, ctx, accepted)) {
        Ok(()) => (
            200,
            json!({"accepted": true, "sha256": digest, "applied": false}),
        ),
        Err(error) => (500, json!({"error": format!("cannot store: {error:#}")})),
    }
}

fn store(dir: &Path, body: &[u8]) -> Result<()> {
    write_private(dir, DESIRED_FILE, body)
}

/// Atomic 0600 write of `dir/name`.
fn write_private(dir: &Path, name: &str, body: &[u8]) -> Result<()> {
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(body)?;
    file.sync_all()?;
    std::fs::rename(&tmp, dir.join(name))?;
    Ok(())
}

/// One audit event (ADR-0024). Who (`actor`: token fingerprint), on what (`target`: node and
/// systemd instance), which `resource`, the outcome, the request hash, a short reason and the
/// durable receipt it produced. Never holds secrets, tokens, keys or payload.
struct AuditEvent<'a> {
    action: &'a str,
    result: &'a str,
    resource: String,
    node: Option<&'a str>,
    instance: Option<&'a str>,
    sha256: &'a str,
    detail: &'a str,
    receipt_id: Option<&'a str>,
}

/// Append one audit line and flush it to disk before the response is sent.
fn audit(dir: &Path, ctx: &Ctx, e: AuditEvent<'_>) -> Result<()> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(dir.join(AUDIT_FILE))?;
    let line = json!({
        "audit_version": 2,
        "ts": ts,
        "actor": ctx.actor(),
        "action": e.action,
        "target": {
            "node": e.node.unwrap_or(ctx.config.ids().0),
            "instance": e.instance.or(ctx.instance.as_deref()),
        },
        "resource": e.resource,
        "result": e.result,
        "sha256": e.sha256,
        "detail": e.detail,
        "receipt_id": e.receipt_id,
    });
    writeln!(file, "{line}")?;
    file.sync_data()?;
    Ok(())
}

async fn listener_accepting(config: &RuntimeConfig) -> bool {
    match config.listen_addr().parse::<SocketAddr>() {
        Ok(addr) => matches!(
            timeout(PROBE_TIMEOUT, TcpStream::connect(addr)).await,
            Ok(Ok(_))
        ),
        Err(_) => false,
    }
}

async fn status(ctx: &Ctx) -> Value {
    let config = &ctx.config;
    let listening = listener_accepting(config).await;
    json!({
        "agent": "baft-tunnel",
        "version": env!("CARGO_PKG_VERSION"),
        "engine": config.engine_name(),
        "config": config.support_report(),
        "listener_accepting": listening,
        "tunnel": tunnel_status(ctx, listening).await,
    })
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// systemd state of the instance and its main PID, when the agent knows the instance.
async fn unit_view(ctx: &Ctx) -> (Value, Option<u32>) {
    let Some(name) = ctx.instance.clone() else {
        return (
            json!({"result": "unknown", "reason": "the agent was started without --instance"}),
            None,
        );
    };
    use crate::apply::Units;
    match tokio::task::spawn_blocking(move || crate::apply::SystemUnits::default().state(&name))
        .await
    {
        Ok(st) => {
            let result = match st.active.as_str() {
                "active" => "active",
                "unknown" | "" => "unknown",
                _ => "inactive",
            };
            (
                json!({"result": result, "active_state": st.active, "sub_state": st.sub, "restarts": st.restarts}),
                (st.main_pid > 0).then_some(st.main_pid),
            )
        }
        Err(_) => (json!({"result": "unknown"}), None),
    }
}

fn stored_diagnostics(ctx: &Ctx) -> Option<Value> {
    let path = ctx.state_dir.as_ref()?.join(DIAGNOSTICS_FILE);
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

const INTEGRITY_UNKNOWN: &str = "payload integrity is measured only by an end-to-end echo probe (Verify/LAB); the agent never reads tunnel payload";

/// End-to-end tunnel state as separate evidence tiers (ADR-0024). The summary `state` is the
/// highest tier actually observed; it never says "ready" and never implies integrity.
async fn tunnel_status(ctx: &Ctx, listening: bool) -> Value {
    let now = unix_now();
    let (unit, pid) = unit_view(ctx).await;
    let ev = ctx.evidence(pid);
    let samples = ctx.traffic_samples();
    let last_bidi = crate::observe::last_bidirectional(&samples);
    let traffic_recent = last_bidi.is_some_and(|t| now.saturating_sub(t) <= DATA_WINDOW_S);
    let counters_age = ev
        .counters
        .as_ref()
        .and_then(|c| c["sampled_unix"].as_u64())
        .map(|t| now.saturating_sub(t));
    let has_listener = !ctx.config.listen_addr().is_empty();
    let session = ev.session.is_some();
    let running = unit["result"] == "active"
        || counters_age.is_some_and(|a| a <= 3 * crate::session_status::COUNTER_SECONDS)
        || (has_listener && listening);
    let state = if session && traffic_recent {
        "carrying_traffic"
    } else if session {
        "session_authorized"
    } else if running {
        "running_no_session"
    } else if ctx.runtime_dir.is_none() && ctx.instance.is_none() {
        "unknown"
    } else {
        "down"
    };
    let stored = stored_diagnostics(ctx);
    json!({
        "state": state,
        "observed_unix": now,
        "tiers": {
            "unit": unit,
            "listener": {"result": if !has_listener { "not_applicable" } else if listening { "pass" } else { "fail" }},
            "authorized_session": match &ev.session {
                Some(s) => json!({
                    "result": "observed",
                    "peer_id": s["peer_id"],
                    "direction": s["direction"],
                    "sessions": s["authorized_sessions"],
                    "since_unix": s["since_unix"],
                }),
                None if ctx.runtime_dir.is_none() => json!({"result": "unknown", "reason": "no unit runtime evidence (--instance not set)"}),
                None => json!({"result": "not_observed"}),
            },
            "data_path": json!({
                "result": if !session { "unknown" } else if traffic_recent { "traffic_observed" } else { "no_recent_traffic" },
                "window_s": DATA_WINDOW_S,
                "last_bidirectional_unix": last_bidi,
            }),
            "integrity": {"result": "unknown", "reason": INTEGRITY_UNKNOWN},
        },
        "last_failure": ev.failure,
        "last_verified_unix": stored.as_ref().and_then(|d| d["verified_unix"].as_u64()),
        "last_diagnostics_unix": stored.as_ref().and_then(|d| d["at_unix"].as_u64()),
    })
}

/// Traffic counters of the running unit with units, sampling interval, provenance and freshness.
fn traffic(ctx: &Ctx) -> Value {
    let now = unix_now();
    let Some(c) = ctx.evidence(None).counters else {
        return json!({
            "available": false,
            "result": "unknown",
            "reason": if ctx.runtime_dir.is_none() {
                "the agent was started without --instance"
            } else {
                "the unit publishes no live counters (not running, or an older version)"
            },
        });
    };
    let samples = ctx.traffic_samples();
    let rates = crate::observe::rates(&samples);
    let sampled = c["sampled_unix"].as_u64().unwrap_or(0);
    let age = now.saturating_sub(sampled);
    json!({
        "available": true,
        "source": "unit counters: relayed payload bytes; excludes TLS/Noise and multiplexing overhead",
        "units": "bytes",
        "rate_units": "bytes_per_second",
        "sampling_interval_s": crate::session_status::COUNTER_SECONDS,
        "agent_interval_s": SAMPLE_SECONDS,
        "pid": c["pid"],
        "started_unix": c["started_unix"],
        "sampled_unix": sampled,
        "age_s": age,
        "fresh": age <= 3 * crate::session_status::COUNTER_SECONDS,
        "bytes_from_peer": c["bytes_from_peer"],
        "bytes_to_peer": c["bytes_to_peer"],
        "streams_total": c["streams_total"],
        "sessions_total": c["sessions_total"],
        "rate": rates.last().cloned(),
        "history": rates,
    })
}

fn get_diagnostics(ctx: &Ctx) -> (u16, Value) {
    match stored_diagnostics(ctx) {
        Some(v) => (200, v),
        None => (404, json!({"error": "no diagnostics stored"})),
    }
}

fn tier(id: &str, result: &str, source: &str, evidence: String) -> Value {
    json!({"id": id, "result": result, "source": source, "evidence": evidence})
}

const STAGES: [&str; 4] = ["tcp", "secure_channel", "identity", "authorization"];

/// Peer-aware diagnostics (ADR-0024): TCP reachability, secure channel, identity, authorization,
/// session and data path are separate tiers, each with its own source. A tier is `pass` only on
/// direct evidence; otherwise it is `fail` (a recorded failure at that stage) or `unknown`.
async fn run_diagnostics(ctx: &Ctx) -> Value {
    let now = unix_now();
    let (unit, pid) = unit_view(ctx).await;
    let ev = ctx.evidence(pid);
    let samples = ctx.traffic_samples();
    let session = ev.session.as_ref();
    let since = session.and_then(|s| s["since_unix"].as_u64()).unwrap_or(0);
    let failure = ev
        .failure
        .as_ref()
        .filter(|f| session.is_none() || f["at_unix"].as_u64().unwrap_or(0) > since);
    let failed_at = failure.and_then(|f| f["at_unix"].as_u64()).unwrap_or(0);
    let failed = failure
        .and_then(|f| f["stage"].as_str())
        .and_then(|s| STAGES.iter().position(|o| *o == s));
    let noise = ctx.config.engine_name() == "noise-tcp";
    let mut tiers = Vec::new();

    tiers.push(match (session, ctx.config.remote()) {
        (Some(_), _) => tier(
            "tcp_reachability",
            "pass",
            "unit_session",
            "an authenticated peer session exists".into(),
        ),
        (None, Some(remote)) => match remote.parse::<SocketAddr>() {
            Ok(addr) => {
                let c = probe_peer(&addr.ip().to_string(), addr.port()).await;
                let result = match c["result"].as_str() {
                    Some("pass") => "pass",
                    Some("unknown") | None => "unknown",
                    _ => "fail",
                };
                tier(
                    "tcp_reachability",
                    result,
                    "active_tcp_probe",
                    format!(
                        "{} (who answers is not verified)",
                        c["evidence"].as_str().unwrap_or("")
                    ),
                )
            }
            Err(_) => tier(
                "tcp_reachability",
                "unknown",
                "config",
                "the peer address is not an IP socket address; not probed".into(),
            ),
        },
        (None, None) => match failed {
            // Only a client that got past the secure channel (a CA-issued certificate or the
            // enrolled Noise key) counts; an anonymous connection can come from anyone.
            Some(i) if i >= 2 => tier(
                "tcp_reachability",
                "pass",
                "last_failure",
                format!("an enrolled client reached this node over TCP at {failed_at}; a later stage failed"),
            ),
            _ => tier(
                "tcp_reachability",
                "not_applicable",
                "config",
                "the peer dials this node; run diagnostics on the peer".into(),
            ),
        },
    });
    for (i, id) in STAGES.iter().enumerate().skip(1) {
        let t = if session.is_some() {
            let what = if *id == "identity" && noise {
                "the pinned Noise static key (the peer identity) was proven by the handshake"
            } else {
                "the unit records a session only after this stage succeeded"
            };
            tier(id, "pass", "unit_session", what.into())
        } else {
            match failed {
                // An inbound handshake failure has no identified sender yet (a scanner, or this
                // node's own peer's TCP probe, looks the same), so it is not blamed on the peer.
                Some(f) if f == i && f == 1 && ctx.config.remote().is_none() => tier(
                    id,
                    "unknown",
                    "last_failure",
                    format!(
                        "an inbound connection failed the handshake at {failed_at}; the sender is not identified"
                    ),
                ),
                Some(f) if f == i => tier(
                    id,
                    "fail",
                    "last_failure",
                    format!("the latest connection attempt failed at this stage at {failed_at}"),
                ),
                Some(f) if f > i => tier(
                    id,
                    "pass",
                    "last_failure",
                    format!("passed in the attempt at {failed_at}; a later stage failed"),
                ),
                Some(_) => tier(
                    id,
                    "unknown",
                    "last_failure",
                    "not reached: an earlier stage failed".into(),
                ),
                None => tier(
                    id,
                    "unknown",
                    "none",
                    "no authorized session and no recorded attempt".into(),
                ),
            }
        };
        tiers.push(t);
    }
    let running = unit["result"] == "active" || ev.counters.is_some();
    tiers.push(match session {
        Some(s) => tier(
            "session",
            "pass",
            "unit_session",
            format!(
                "{} authorized session(s) with peer {} since {}",
                s["authorized_sessions"],
                s["peer_id"].as_str().unwrap_or("?"),
                since
            ),
        ),
        None if running => tier(
            "session",
            "fail",
            "unit_session",
            "the unit runs but has no authorized session".into(),
        ),
        None => tier(
            "session",
            "unknown",
            "none",
            "the unit's runtime evidence is not available".into(),
        ),
    });
    let last_bidi = crate::observe::last_bidirectional(&samples);
    tiers.push(match (session, last_bidi) {
        (None, _) => tier("data_path", "unknown", "none", "not reached: no authorized session".into()),
        (Some(_), Some(t)) if now.saturating_sub(t) <= DATA_WINDOW_S => tier("data_path", "pass", "unit_counters", format!("payload bytes were relayed in both directions at {t}")),
        (Some(_), _) => tier("data_path", "unknown", "unit_counters", format!("no payload in both directions within {DATA_WINDOW_S} s; send traffic through the tunnel and run again")),
    });
    tiers.push(tier(
        "integrity",
        "unknown",
        "none",
        INTEGRITY_UNKNOWN.into(),
    ));

    let mut highest = Value::Null;
    for t in &tiers {
        match t["result"].as_str() {
            Some("pass") => highest = t["id"].clone(),
            Some("not_applicable") => {}
            _ => break,
        }
    }
    let verified = highest == "data_path";
    let previous = stored_diagnostics(ctx).and_then(|d| d["verified_unix"].as_u64());
    let report = json!({
        "diagnostics_version": 1,
        "at_unix": now,
        "engine": ctx.config.engine_name(),
        "unit": unit,
        "tiers": tiers,
        "highest_passed_tier": highest,
        "verified": verified,
        "verified_unix": if verified { Some(now) } else { previous },
        "note": "each tier is separate evidence; TCP reachability alone never means a working tunnel, and integrity stays unknown",
    });
    if let Some(dir) = &ctx.state_dir {
        let _ = write_private(dir, DIAGNOSTICS_FILE, report.to_string().as_bytes());
        let digest = hex::encode(Sha256::digest(report.to_string().as_bytes()));
        let _ = audit(
            dir,
            ctx,
            AuditEvent {
                action: "diagnostics",
                result: highest.as_str().unwrap_or("none"),
                resource: "diagnostics".into(),
                node: None,
                instance: None,
                sha256: &digest,
                detail: if verified { "data path observed" } else { "" },
                receipt_id: None,
            },
        );
    }
    report
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn respond(stream: &mut TcpStream, code: u16, body: &Value) -> Result<()> {
    let reason = match code {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        _ => "Request Header Fields Too Large",
    };
    let payload = serde_json::to_vec(body)?;
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    timeout(IO_TIMEOUT, async {
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(&payload).await?;
        stream.shutdown().await
    })
    .await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "tttttttttttttttttttttttttttttttttttttttt";

    fn sample() -> RuntimeConfig {
        serde_json::from_value(json!({
            "mode": "forward", "listen": "127.0.0.1:1", "remote": "192.0.2.1:8443",
            "server_name": "x", "local_id": "a", "peer_id": "b", "tunnel_id": "t",
            "direction": "direct", "peer_fingerprint": "00", "key": "/secret/key.pem"
        }))
        .unwrap()
    }

    fn state_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("baft-agent-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    async fn call(
        dir: Option<&Path>,
        method: &str,
        path: &str,
        auth: Option<&str>,
        body: &str,
    ) -> (u16, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(serve(
            listener,
            Arc::new(Ctx::new(sample(), TOKEN.into(), dir.map(Path::to_path_buf))),
        ));
        let mut s = TcpStream::connect(addr).await.unwrap();
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n",
            body.len()
        );
        if let Some(a) = auth {
            req += &format!("Authorization: Bearer {a}\r\n");
        }
        s.write_all(format!("{req}\r\n{body}").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        task.abort();
        (out[9..12].parse().unwrap(), out)
    }

    #[tokio::test]
    async fn status_requires_token_and_redacts() {
        assert_eq!(call(None, "GET", "/v1/status", None, "").await.0, 401);
        assert_eq!(
            call(None, "GET", "/v1/status", Some("wrong"), "").await.0,
            401
        );
        let (code, body) = call(None, "GET", "/v1/status", Some(TOKEN), "").await;
        assert_eq!(code, 200);
        assert!(body.contains("\"listener_accepting\":false"));
        assert!(!body.contains("/secret"));
        assert!(!body.contains("192.0.2.1"));
    }

    #[tokio::test]
    async fn health_open_and_other_routes_rejected() {
        assert_eq!(call(None, "GET", "/v1/health", None, "").await.0, 200);
        assert_eq!(call(None, "GET", "/v1/other", None, "").await.0, 404);
        assert_eq!(
            call(None, "POST", "/v1/status", Some(TOKEN), "").await.0,
            405
        );
    }

    #[tokio::test]
    async fn desired_state_needs_token_and_state_dir() {
        let dir = state_dir("auth");
        assert_eq!(
            call(Some(&dir), "PUT", "/v1/desired", None, "{}").await.0,
            401
        );
        assert_eq!(
            call(None, "PUT", "/v1/desired", Some(TOKEN), "{}").await.0,
            404
        );
        assert!(!dir.join(DESIRED_FILE).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn invalid_desired_state_is_rejected_unchanged_and_audited() {
        let dir = state_dir("bad");
        std::fs::write(dir.join(DESIRED_FILE), b"previous").unwrap();
        for body in ["not json", r#"{"mode":"serve"}"#] {
            let (code, _) = call(Some(&dir), "PUT", "/v1/desired", Some(TOKEN), body).await;
            assert_eq!(code, 422);
        }
        assert_eq!(std::fs::read(dir.join(DESIRED_FILE)).unwrap(), b"previous");
        let log = std::fs::read_to_string(dir.join(AUDIT_FILE)).unwrap();
        assert_eq!(log.lines().count(), 2);
        assert!(log.lines().all(|l| l.contains("\"rejected\"")));
        assert_eq!(
            std::fs::metadata(dir.join(AUDIT_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn valid_desired_state_is_stored_audited_and_not_applied() {
        let dir = state_dir("ok");
        let key = dir.join("node.noise");
        crate::engine::write_noise_keypair(&key).unwrap();
        let peer = crate::engine::write_noise_keypair(&dir.join("peer.noise")).unwrap();
        let body = json!({
            "mode": "forward", "listen": "127.0.0.1:18999", "remote": "192.0.2.1:8443",
            "local_id": "a", "peer_id": "b", "tunnel_id": "t", "direction": "direct",
            "engine": "noise-tcp", "key": key, "noise_peer_public": peer
        })
        .to_string();
        let (code, out) = call(Some(&dir), "PUT", "/v1/desired", Some(TOKEN), &body).await;
        assert_eq!(code, 200, "{out}");
        assert!(out.contains("\"applied\":false"));
        assert_eq!(
            std::fs::read(dir.join(DESIRED_FILE)).unwrap(),
            body.as_bytes()
        );
        let log = std::fs::read_to_string(dir.join(AUDIT_FILE)).unwrap();
        assert!(log.contains("\"accepted\"") && !log.contains(&peer));
        let (code, out) = call(Some(&dir), "GET", "/v1/desired", Some(TOKEN), "").await;
        assert_eq!(code, 200);
        assert!(!out.contains("192.0.2.1") && !out.contains(&peer));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn panel_is_served_without_token_under_a_strict_csp() {
        let (code, out) = call(None, "GET", "/", None, "").await;
        assert_eq!(code, 200);
        assert!(out.contains("script-src 'self'") && out.contains("frame-ancestors 'none'"));
        assert!(out.contains("<title>BAFT-TUNNEL</title>"));
        for (name, _, _) in PANEL_ASSETS {
            let (code, out) = call(None, "GET", name, None, "").await;
            assert_eq!(code, 200, "{name}");
            assert!(
                !out.contains("innerHTML") && !out.contains("eval("),
                "{name}: panel must only use textContent"
            );
        }
        assert_eq!(call(None, "POST", "/", None, "").await.0, 404);
    }

    #[tokio::test]
    async fn preflight_is_read_only_and_never_turns_unknown_into_pass() {
        let body = plan_body("/nonexistent-baft-dir");
        assert_eq!(
            call(None, "POST", "/v1/setup/preflight", None, &body)
                .await
                .0,
            401
        );
        let (code, out) = call(None, "POST", "/v1/setup/preflight", Some(TOKEN), &body).await;
        assert_eq!(code, 200, "{out}");
        let v: Value = serde_json::from_str(&out[out.find('{').unwrap()..]).unwrap();
        assert_eq!(v["overall"], "unknown", "{out}");
        assert_eq!(v["applied"], false);
        assert_eq!(v["apply_available"], true);
        let result = |id: &str| {
            v["checks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .map(|c| c["result"].as_str().unwrap().to_string())
        };
        assert_eq!(result("planner").as_deref(), Some("pass"));
        assert_eq!(result("directory").as_deref(), Some("warning"));
        for id in ["peer_reachability", "peer_identity", "firewall"] {
            assert_eq!(result(id).as_deref(), Some("unknown"), "{id}");
        }
        assert!(!std::path::Path::new("/nonexistent-baft-dir").exists());
    }

    #[tokio::test]
    async fn preflight_blocks_missing_pki_and_invalid_plans() {
        let dir = std::env::temp_dir().join(format!("baft-pf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let body = json!({
            "engine": "tcp-tls", "role": "serve", "direction": "direct", "node_id": "node-a",
            "peer_id": "node-b", "tunnel_id": "tunnel-1", "service_port": 8080,
            "dir": dir.to_string_lossy()
        })
        .to_string();
        let (_, out) = call(None, "POST", "/v1/setup/preflight", Some(TOKEN), &body).await;
        let v: Value = serde_json::from_str(&out[out.find('{').unwrap()..]).unwrap();
        assert_eq!(v["overall"], "blocked", "{out}");
        assert!(out.contains("missing: node.key, node.crt, ca.crt, ca.crl"));
        let bad = body.replace("node-b", "node-a");
        let (code, out) = call(None, "POST", "/v1/setup/preflight", Some(TOKEN), &bad).await;
        assert_eq!(code, 200);
        assert!(
            out.contains("\"planner\"") && out.contains("\"blocked\""),
            "{out}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn peer_probe_distinguishes_listening_refused_and_carries_evidence_fields() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let open = listener.local_addr().unwrap().port();
        let closed = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let dir = std::env::temp_dir().join(format!("baft-pf-peer-{}", std::process::id()));
        let run = |port: u16| {
            let body = json!({
                "engine": "noise-tcp", "role": "forward", "direction": "direct", "node_id": "node-a",
                "peer_id": "node-b", "tunnel_id": "tunnel-1", "service_port": 8080,
                "tunnel_port": port, "peer_host": "127.0.0.1", "dir": dir.to_string_lossy()
            })
            .to_string();
            async move {
                let (_, out) = call(None, "POST", "/v1/setup/preflight", Some(TOKEN), &body).await;
                let v: Value = serde_json::from_str(&out[out.find('{').unwrap()..]).unwrap();
                v["checks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["id"] == "peer_reachability")
                    .cloned()
                    .unwrap()
            }
        };
        let up = run(open).await;
        assert_eq!(up["result"], "pass", "{up}");
        assert_eq!(up["scope"], "peer");
        assert_eq!(up["source"], "network-probe");
        assert_eq!(up["code"], "pf.peer_reachability.pass");
        assert!(up["checked_at"].as_u64().unwrap() > 0);
        let down = run(closed).await;
        assert_eq!(down["result"], "warning", "{down}");
        assert!(
            down["remediation"]
                .as_str()
                .unwrap()
                .contains("Start the peer")
        );
        assert!(!dir.exists());
    }

    #[tokio::test]
    async fn preflight_blocks_noise_reverse_because_it_is_not_proven() {
        let body = plan_body("/nonexistent-baft-dir").replace("\"direct\"", "\"reverse\"");
        let (_, out) = call(None, "POST", "/v1/setup/preflight", Some(TOKEN), &body).await;
        let v: Value = serde_json::from_str(&out[out.find('{').unwrap()..]).unwrap();
        assert_eq!(v["overall"], "blocked", "{out}");
        let c = v["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "direction_support")
            .unwrap();
        assert_eq!(c["result"], "blocked");
        assert_eq!(c["code"], "pf.direction_support.blocked");
    }

    fn apply_body(dir: &str, key: &str, confirm: bool, direction: &str) -> String {
        json!({
            "stage": "init", "confirm": confirm, "idempotency_key": key,
            "plan": {
                "engine": "noise-tcp", "role": "serve", "direction": direction, "node_id": "node-a",
                "peer_id": "node-b", "tunnel_id": "tunnel-1", "service_port": 8080, "dir": dir
            }
        })
        .to_string()
    }

    #[tokio::test]
    async fn apply_needs_token_state_dir_confirm_and_a_passing_gate() {
        let state = state_dir("apply");
        let work = state_dir("apply-work");
        let dir = work.to_string_lossy().to_string();
        let ok = apply_body(&dir, "key-apply-0001", true, "direct");
        assert_eq!(
            call(Some(&state), "POST", "/v1/setup/apply", None, &ok)
                .await
                .0,
            401
        );
        assert_eq!(
            call(None, "POST", "/v1/setup/apply", Some(TOKEN), &ok)
                .await
                .0,
            404
        );
        let no_confirm = apply_body(&dir, "key-apply-0001", false, "direct");
        assert_eq!(
            call(
                Some(&state),
                "POST",
                "/v1/setup/apply",
                Some(TOKEN),
                &no_confirm
            )
            .await
            .0,
            422
        );
        // Noise Reverse is unproven: the gate refuses and nothing is written.
        let rev = apply_body(&dir, "key-apply-0002", true, "reverse");
        let (code, out) = call(Some(&state), "POST", "/v1/setup/apply", Some(TOKEN), &rev).await;
        assert_eq!(code, 409, "{out}");
        assert!(out.contains("\"result\":\"refused\""));
        assert!(!work.join("node.noise").exists());
        // Direct is applied, verified and receipted; the audit records it without secrets.
        let (code, out) = call(Some(&state), "POST", "/v1/setup/apply", Some(TOKEN), &ok).await;
        assert_eq!(code, 200, "{out}");
        assert!(out.contains("\"result\":\"applied\""));
        assert!(work.join("node.noise").exists());
        let audit = std::fs::read_to_string(state.join("audit.log")).unwrap_or_default();
        assert!(audit.contains("\"action\":\"apply\""), "{audit}");
        let (code, again) = call(Some(&state), "POST", "/v1/setup/apply", Some(TOKEN), &ok).await;
        assert_eq!(code, 200);
        assert!(again.contains("\"replayed\":true"));
    }

    #[tokio::test]
    async fn preflight_accepts_tcp_tls_reverse_as_a_preview_direction() {
        let body = plan_body("/nonexistent-baft-dir")
            .replace("\"direct\"", "\"reverse\"")
            .replace("noise-tcp", "tcp-tls");
        let (_, out) = call(None, "POST", "/v1/setup/preflight", Some(TOKEN), &body).await;
        let v: Value = serde_json::from_str(&out[out.find('{').unwrap()..]).unwrap();
        let c = v["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "direction_support")
            .unwrap();
        assert_eq!(c["result"], "pass", "{out}");
    }

    fn plan_body(dir: &str) -> String {
        json!({
            "engine": "noise-tcp", "role": "serve", "direction": "direct", "node_id": "node-a",
            "peer_id": "node-b", "tunnel_id": "tunnel-1", "service_port": 8080, "dir": dir
        })
        .to_string()
    }

    #[tokio::test]
    async fn wizard_plan_needs_token_returns_commands_and_writes_nothing() {
        let body = plan_body("/etc/baft-tunnel");
        assert_eq!(
            call(None, "POST", "/v1/setup/plan", None, &body).await.0,
            401
        );
        let (code, out) = call(None, "POST", "/v1/setup/plan", Some(TOKEN), &body).await;
        assert_eq!(code, 200, "{out}");
        assert!(out.contains("setup init --engine noise-tcp --role serve --direction direct"));
        assert!(out.contains("\"applied\":false"));
        assert!(!std::path::Path::new("/etc/baft-tunnel/config.json").exists());
    }

    #[tokio::test]
    async fn wizard_plan_for_a_service_instance_names_the_systemd_steps() {
        let body = json!({
            "engine": "noise-tcp", "role": "serve", "direction": "direct", "node_id": "node-a",
            "peer_id": "node-b", "tunnel_id": "tunnel-1", "service_port": 8080, "instance": "edge"
        })
        .to_string();
        let (code, out) = call(None, "POST", "/v1/setup/plan", Some(TOKEN), &body).await;
        assert_eq!(code, 200, "{out}");
        assert!(out.contains("--instance 'edge'"));
        assert!(out.contains("chown baft-tunnel:baft-tunnel '/etc/baft-tunnel/node.noise'"));
        assert!(out.contains("chgrp baft-tunnel '/etc/baft-tunnel/edge.json'"));
        assert!(out.contains("systemctl enable --now baft-tunnel@edge"));
        assert!(out.contains("/etc/baft-tunnel/edge.json"));
        let bad = body.replace("edge", "../edge");
        assert_eq!(
            call(None, "POST", "/v1/setup/plan", Some(TOKEN), &bad)
                .await
                .0,
            422
        );
    }

    #[tokio::test]
    async fn wizard_plan_rejects_bad_input_and_shell_metacharacters() {
        for body in [
            plan_body("/tmp/x'; touch /tmp/pwned; echo '"),
            plan_body("relative/dir"),
            r#"{"engine":"noise-tcp"}"#.to_string(),
            r#"{"engine":"noise-tcp","unknown":1}"#.to_string(),
        ] {
            let (code, _) = call(None, "POST", "/v1/setup/plan", Some(TOKEN), &body).await;
            assert_eq!(code, 422, "{body}");
        }
    }

    async fn get_json(ctx: &Ctx, path: &str) -> (u16, Value) {
        let head = format!("GET {path} HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n");
        route(&head, b"", ctx).await
    }

    #[tokio::test]
    async fn history_is_bounded_and_reports_real_probes() {
        let ctx = Ctx::new(sample(), TOKEN.into(), None);
        for i in 0..(HISTORY_LEN as u64 + 10) {
            ctx.record(Sample {
                ts: i,
                up: i % 2 == 0,
                rtt_ms: (i % 2 == 0).then_some(1.5),
            });
        }
        let (code, body) = get_json(&ctx, "/v1/history").await;
        assert_eq!(code, 200);
        let samples = body["samples"].as_array().unwrap();
        assert_eq!(samples.len(), HISTORY_LEN);
        assert_eq!(samples[0]["t"], 10, "oldest samples are dropped first");
        let probe = ctx.probe().await;
        assert!(
            !probe.up && probe.rtt_ms.is_none(),
            "nothing listens on port 1"
        );
    }

    #[tokio::test]
    async fn security_audit_and_engines_endpoints() {
        let dir = state_dir("dash");
        let ctx = Ctx::new(sample(), TOKEN.into(), Some(dir.clone()));
        let (code, body) = get_json(&ctx, "/v1/security").await;
        assert_eq!(code, 200);
        assert_eq!(body["tls"], true, "sample() is a tcp-tls config");
        assert!(
            body["crl_seconds_remaining"].is_null(),
            "unreadable CRL is null, not invented"
        );
        audit(
            &dir,
            &ctx,
            AuditEvent {
                action: "put_desired",
                result: "accepted",
                resource: "desired_state".into(),
                node: None,
                instance: None,
                sha256: "abc",
                detail: "",
                receipt_id: None,
            },
        )
        .unwrap();
        let (_, body) = get_json(&ctx, "/v1/audit").await;
        assert_eq!(body["entries"].as_array().unwrap().len(), 1);
        let (_, body) = get_json(&ctx, "/v1/engines").await;
        assert_eq!(body["engines"].as_array().unwrap().len(), 2);
        let bare = Ctx::new(sample(), TOKEN.into(), None);
        assert_eq!(get_json(&bare, "/v1/audit").await.0, 404);
        let unauth = route("GET /v1/history HTTP/1.1\r\n\r\n", b"", &bare).await;
        assert_eq!(unauth.0, 401);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn serve_config() -> RuntimeConfig {
        serde_json::from_value(json!({
            "mode": "serve", "listen": "127.0.0.1:1", "target": "127.0.0.1:2",
            "local_id": "abroad", "peer_id": "iran", "tunnel_id": "t",
            "direction": "direct", "peer_fingerprint": "00", "key": "/secret/key.pem"
        }))
        .unwrap()
    }

    async fn post_json(ctx: &Ctx, path: &str) -> (u16, Value) {
        let head = format!("POST {path} HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n");
        route(&head, b"", ctx).await
    }

    #[tokio::test]
    async fn diagnostics_status_and_traffic_report_each_tier_from_unit_evidence() {
        use crate::observe::TrafficSample;
        let state = state_dir("diag-state");
        let run = state_dir("diag-run");
        let me = std::process::id();
        let now = unix_now();
        std::fs::write(
            run.join("session.json"),
            json!({"state": "authorized", "authorized_sessions": 1, "peer_id": "iran",
                   "direction": "direct", "pid": me, "since_unix": now - 30})
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            run.join("counters.json"),
            json!({"counters_version": 1, "pid": me, "started_unix": now - 60, "sampled_unix": now,
                   "bytes_from_peer": 100, "bytes_to_peer": 50, "streams_total": 1, "sessions_total": 1})
            .to_string(),
        )
        .unwrap();
        let ctx = Ctx::new(serve_config(), TOKEN.into(), Some(state.clone()))
            .with_unit(None, Some(run.clone()));
        for (t, a, b) in [(now - 10, 0, 0), (now - 5, 100, 50)] {
            ctx.record_traffic(TrafficSample {
                t,
                pid: u64::from(me),
                from_peer: a,
                to_peer: b,
            });
        }

        let (code, d) = post_json(&ctx, "/v1/diagnostics").await;
        assert_eq!(code, 200);
        let results: Vec<(&str, &str)> = d["tiers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| (t["id"].as_str().unwrap(), t["result"].as_str().unwrap()))
            .collect();
        assert_eq!(
            results,
            [
                ("tcp_reachability", "pass"),
                ("secure_channel", "pass"),
                ("identity", "pass"),
                ("authorization", "pass"),
                ("session", "pass"),
                ("data_path", "pass"),
                ("integrity", "unknown"),
            ]
        );
        assert_eq!(d["highest_passed_tier"], "data_path");
        assert_eq!(d["verified"], true);
        assert_eq!(
            get_json(&ctx, "/v1/diagnostics").await.1["at_unix"],
            d["at_unix"]
        );
        let audit = std::fs::read_to_string(state.join(AUDIT_FILE)).unwrap();
        let line: Value = serde_json::from_str(audit.lines().last().unwrap()).unwrap();
        assert_eq!(line["action"], "diagnostics");
        assert_eq!(line["actor"]["kind"], "api_token");
        assert_eq!(line["target"]["node"], "abroad");
        assert_eq!(line["resource"], "diagnostics");
        assert!(
            !audit.contains(TOKEN),
            "the token never reaches the audit log"
        );

        let (_, st) = get_json(&ctx, "/v1/status").await;
        assert_eq!(st["tunnel"]["state"], "carrying_traffic");
        assert_eq!(
            st["tunnel"]["tiers"]["authorized_session"]["peer_id"],
            "iran"
        );
        assert_eq!(st["tunnel"]["tiers"]["integrity"]["result"], "unknown");
        assert_eq!(st["tunnel"]["last_verified_unix"], d["verified_unix"]);
        let (_, tr) = get_json(&ctx, "/v1/traffic").await;
        assert_eq!(tr["available"], true);
        assert_eq!(tr["units"], "bytes");
        assert_eq!(tr["rate"]["from_peer_bps"], 20.0);
        assert_eq!(tr["fresh"], true);

        // The session ends and the peer is then refused at the identity stage.
        std::fs::remove_file(run.join("session.json")).unwrap();
        std::fs::write(
            run.join("last_failure.json"),
            json!({"stage": "identity", "at_unix": now, "pid": me}).to_string(),
        )
        .unwrap();
        let (_, d) = post_json(&ctx, "/v1/diagnostics").await;
        let r: Vec<&str> = d["tiers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["result"].as_str().unwrap())
            .collect();
        assert_eq!(
            r,
            [
                "pass", "pass", "fail", "unknown", "fail", "unknown", "unknown"
            ]
        );
        assert_eq!(d["highest_passed_tier"], "secure_channel");
        // An anonymous handshake failure on the listening node is not blamed on the peer.
        std::fs::write(
            run.join("last_failure.json"),
            json!({"stage": "secure_channel", "at_unix": now, "pid": me}).to_string(),
        )
        .unwrap();
        let (_, d2) = post_json(&ctx, "/v1/diagnostics").await;
        assert_eq!(d2["tiers"][0]["result"], "not_applicable");
        assert_eq!(d2["tiers"][1]["result"], "unknown");
        assert_eq!(d["verified"], false);
        assert!(
            d["verified_unix"].is_u64(),
            "the last verified time is kept"
        );
        let (_, st) = get_json(&ctx, "/v1/status").await;
        assert_eq!(st["tunnel"]["state"], "running_no_session");
        assert_eq!(st["tunnel"]["last_failure"]["stage"], "secure_channel");
        std::fs::remove_dir_all(&state).unwrap();
        std::fs::remove_dir_all(&run).unwrap();
    }

    #[tokio::test]
    async fn without_unit_evidence_status_and_traffic_stay_unknown() {
        let ctx = Ctx::new(serve_config(), TOKEN.into(), None);
        let (_, st) = get_json(&ctx, "/v1/status").await;
        assert_eq!(st["tunnel"]["state"], "unknown");
        assert_eq!(
            st["tunnel"]["tiers"]["authorized_session"]["result"],
            "unknown"
        );
        let (_, tr) = get_json(&ctx, "/v1/traffic").await;
        assert_eq!(tr["available"], false);
        assert_eq!(get_json(&ctx, "/v1/diagnostics").await.0, 404);
        let (_, d) = post_json(&ctx, "/v1/diagnostics").await;
        assert_eq!(d["tiers"][0]["result"], "not_applicable");
        assert_eq!(d["tiers"][4]["result"], "unknown");
        assert_eq!(d["verified"], false);
    }

    #[test]
    fn listen_must_be_loopback() {
        assert!(parse_listen("127.0.0.1:9000").is_ok());
        assert!(parse_listen("0.0.0.0:9000").is_err());
        assert!(parse_listen("127.0.0.1:0").is_err());
    }

    #[test]
    fn token_is_private_never_overwritten_and_checked() {
        let dir = state_dir("token");
        let p = dir.join("token");
        write_token(&p).unwrap();
        assert!(write_token(&p).is_err());
        assert_eq!(read_token(&p).unwrap().len(), 64);
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_token(&p).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
