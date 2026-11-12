//! Local status API (P3-002 slice A1; P1 operations evidence, ADR-0024).
//!
//! Loopback only, bearer-token protected. It reports the redacted support report, the tunnel's
//! end-to-end evidence tiers, traffic counters and peer-aware diagnostics read from the running
//! unit. It never starts, stops or changes a tunnel except through the confirmed Apply stages.

use std::{
    io::{Read, Write},
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
const MAX_BODY_BYTES: usize = 512 * 1024;
const DESIRED_FILE: &str = "desired.json";
const AUDIT_FILE: &str = "audit.log";
/// Previous generation after rotation (SA-08). Only one is kept.
const AUDIT_ROTATED: &str = "audit.log.1";
/// `audit.log` is rotated before an append that would find it at or above this size.
const AUDIT_MAX_BYTES: u64 = 4 * 1024 * 1024;
/// `GET /v1/audit` reads at most this many bytes from the end of each file.
const AUDIT_TAIL_BYTES: u64 = 256 * 1024;
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
    /// `None` in setup mode: the bound instance has no configuration yet. Only the setup routes
    /// (and a reduced status) answer; the agent exits once the configuration appears so systemd
    /// restarts it with the configuration loaded.
    pub config: Option<RuntimeConfig>,
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
    /// systemd access for the bound instance: state reads, and start/stop/enable/disable requests
    /// through the root control helper (ADR-0023). Tests substitute a fake.
    units: Arc<dyn crate::apply::Units + Send + Sync>,
    /// Where `NAME.json` lives; lifecycle actions only manage instances here.
    config_dir: PathBuf,
    /// Root setup helper (ADR-0026) for `init`/`join` of the bound instance. Tests use a fake.
    setup: Arc<dyn crate::setup_helper::SetupHelper + Send + Sync>,
    /// Root PKI helper (ADR-0027) for host-local CSR create/read before tcp-tls Init.
    pki: Arc<dyn crate::pki_helper::PkiHelperClient + Send + Sync>,
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
        Self::with_config(Some(config), token, state_dir)
    }

    /// `config: None` starts the agent in setup mode (see `Ctx::config`).
    pub fn with_config(
        config: Option<RuntimeConfig>,
        token: String,
        state_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            config,
            token,
            state_dir,
            lock: Mutex::new(()),
            history: std::sync::Mutex::new(std::collections::VecDeque::new()),
            instance: None,
            runtime_dir: None,
            traffic: std::sync::Mutex::new(std::collections::VecDeque::new()),
            units: Arc::new(crate::apply::SystemUnits::default()),
            config_dir: PathBuf::from("/etc/baft-tunnel"),
            setup: Arc::new(crate::setup_helper::SystemSetupHelper::default()),
            pki: Arc::new(crate::pki_helper::SystemPkiClient::default()),
        }
    }

    #[cfg(test)]
    fn with_setup_helper(
        mut self,
        helper: Arc<dyn crate::setup_helper::SetupHelper + Send + Sync>,
    ) -> Self {
        self.setup = helper;
        self
    }

    #[cfg(test)]
    fn with_pki_helper(
        mut self,
        helper: Arc<dyn crate::pki_helper::PkiHelperClient + Send + Sync>,
    ) -> Self {
        self.pki = helper;
        self
    }

    /// Setup stages for the bound instance go through the root helper (installed layout).
    fn via_helper(&self, plan: &crate::setup::PlanRequest) -> bool {
        self.instance.is_some() && plan.instance == self.instance && plan.dir == self.config_dir
    }

    /// `NAME.json` of the bound instance.
    fn instance_config(&self) -> Option<PathBuf> {
        self.instance
            .as_ref()
            .map(|n| self.config_dir.join(format!("{n}.json")))
    }

    #[cfg(test)]
    fn with_units(
        mut self,
        units: Arc<dyn crate::apply::Units + Send + Sync>,
        config_dir: PathBuf,
    ) -> Self {
        self.units = units;
        self.config_dir = config_dir;
        self
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
        let listen = self
            .config
            .as_ref()
            .map(|c| c.listen_addr().to_string())
            .unwrap_or_default();
        let up = match listen.parse::<SocketAddr>() {
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
        if ctx.config.is_none() {
            // Setup mode: once `setup join` (panel or CLI) wrote NAME.json, exit so systemd
            // (Restart=always) starts the agent again with the configuration loaded.
            if ctx.instance_config().is_some_and(|p| p.is_file()) {
                eprintln!("configuration appeared; restarting the agent to load it");
                tokio::time::sleep(Duration::from_secs(1)).await;
                std::process::exit(0);
            }
            continue;
        }
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
    config: Option<RuntimeConfig>,
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
    if config.is_none() && instance.is_none() {
        bail!("setup mode (no configuration yet) needs --instance");
    }
    let ctx = Arc::new(Ctx::with_config(config, token, state_dir).with_unit(instance, runtime_dir));
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

/// Host names a browser may use to reach the loopback agent (any port, so `ssh -L` keeps working).
/// Anything else is refused before routing: a DNS-rebinding page names its own domain here.
fn allowed_host(head: &str) -> bool {
    let Some(host) = header(head, "host") else {
        return true; // non-browser clients (HTTP/1.0, tests); browsers always send Host
    };
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split_once(']').map_or(rest, |(n, _)| n)
    } else {
        host.rsplit_once(':').map_or(host, |(n, _)| n)
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

async fn handle(mut stream: TcpStream, ctx: &Ctx) -> Result<()> {
    // The whole request must arrive within one I/O timeout, so a client trickling bytes cannot
    // hold one of the few connection slots for the full handler time.
    let read = timeout(IO_TIMEOUT, read_request(&mut stream)).await;
    let Ok(read) = read else {
        return Ok(());
    };
    let Some((buf, head_end, length)) = read? else {
        return Ok(());
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    if length == usize::MAX {
        return respond(&mut stream, 431, &json!({"error": "request too large"})).await;
    }
    if length > MAX_BODY_BYTES {
        return respond(&mut stream, 413, &json!({"error": "body too large"})).await;
    }
    if !allowed_host(&head) {
        return respond(&mut stream, 421, &json!({"error": "misdirected request"})).await;
    }
    let body = &buf[head_end..head_end + length];
    if let Some((kind, text)) = static_asset(&head) {
        return respond_static(&mut stream, kind, text).await;
    }
    let (status, out) = route(&head, body, ctx).await;
    respond(&mut stream, status, &out).await
}

/// Read one request. `None` = the client closed early. A too-large head is reported as
/// `length == usize::MAX`, a too-large body by its declared length (both answered by the caller).
async fn read_request(stream: &mut TcpStream) -> Result<Option<(Vec<u8>, usize, usize)>> {
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() >= MAX_REQUEST_BYTES {
            return Ok(Some((buf, 0, usize::MAX)));
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let length = header(&String::from_utf8_lossy(&buf[..head_end]), "content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if length > MAX_BODY_BYTES {
        return Ok(Some((buf, head_end, length)));
    }
    while buf.len() < head_end + length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(Some((buf, head_end, length)))
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
            | "/v1/verify"
            | "/v1/tunnel/lifecycle"
            | "/v1/setup/bundle"
            | "/v1/setup/bundle/inspect"
            | "/v1/pki/csr/create"
            | "/v1/pki/csr/read"
            | "/v1/pki/certificate/install"
    ) {
        return (404, json!({"error": "not found"}));
    }
    let presented = header(head, "authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !constant_time_eq(presented.as_bytes(), ctx.token.as_bytes()) {
        return (401, json!({"error": "unauthorized"}));
    }
    let Some(config) = ctx.config.as_ref() else {
        return setup_mode_route(method, path, body, ctx).await;
    };
    match (method, path) {
        ("GET", "/v1/status") => (200, status(ctx, config).await),
        ("GET", "/v1/traffic") => (200, traffic(ctx)),
        ("GET", "/v1/diagnostics") => get_diagnostics(ctx),
        ("POST", "/v1/diagnostics") => (200, run_diagnostics(ctx, config).await),
        ("POST", "/v1/verify") => verify_integrity(ctx, config, body).await,
        ("GET", "/v1/history") => (200, history(ctx)),
        ("GET", "/v1/security") => (200, security(config)),
        ("GET", "/v1/audit") => get_audit(ctx),
        ("GET", "/v1/engines") => (200, engines()),
        ("GET", "/v1/desired") => get_desired(ctx),
        ("PUT", "/v1/desired") => put_desired(ctx, body).await,
        ("POST", "/v1/setup/plan") => setup_plan(body),
        ("POST", "/v1/setup/preflight") => preflight(ctx, body).await,
        ("POST", "/v1/setup/apply") => setup_apply(ctx, body).await,
        ("POST", "/v1/tunnel/lifecycle") => tunnel_lifecycle(ctx, body).await,
        ("GET", "/v1/setup/bundle") => own_bundle(ctx),
        ("POST", "/v1/setup/bundle/inspect") => inspect_bundle(ctx, body),
        ("POST", "/v1/pki/csr/create") => (
            409,
            json!({"error": "CSR generation is a pre-Init operation; this instance is already configured"}),
        ),
        ("POST", "/v1/pki/csr/read") => {
            pki_csr_action(ctx, body, crate::pki_helper::PkiOperation::ReadCsr).await
        }
        ("POST", "/v1/pki/certificate/install") => (
            409,
            json!({"error": "certificate import is a pre-Init operation; this instance is already configured"}),
        ),
        _ => (405, json!({"error": "method not allowed"})),
    }
}

/// Setup mode: the instance has no configuration yet. Only what pairing needs answers.
async fn setup_mode_route(method: &str, path: &str, body: &[u8], ctx: &Ctx) -> (u16, Value) {
    match (method, path) {
        ("GET", "/v1/status") => (200, setup_status(ctx)),
        ("GET", "/v1/engines") => (200, engines()),
        ("GET", "/v1/audit") => get_audit(ctx),
        ("POST", "/v1/setup/plan") => setup_plan(body),
        ("POST", "/v1/setup/preflight") => preflight(ctx, body).await,
        ("POST", "/v1/setup/apply") => setup_apply(ctx, body).await,
        ("GET", "/v1/setup/bundle") => own_bundle(ctx),
        ("POST", "/v1/setup/bundle/inspect") => inspect_bundle(ctx, body),
        ("POST", "/v1/pki/csr/create") => {
            pki_csr_action(ctx, body, crate::pki_helper::PkiOperation::CreateCsr).await
        }
        ("POST", "/v1/pki/csr/read") => {
            pki_csr_action(ctx, body, crate::pki_helper::PkiOperation::ReadCsr).await
        }
        ("POST", "/v1/pki/certificate/install") => pki_certificate_action(ctx, body).await,
        (
            "GET" | "PUT" | "POST",
            "/v1/history"
            | "/v1/security"
            | "/v1/traffic"
            | "/v1/diagnostics"
            | "/v1/desired"
            | "/v1/tunnel/lifecycle",
        ) => (
            409,
            json!({"error": "setup pending: this instance has no configuration yet", "setup_pending": true}),
        ),
        _ => (405, json!({"error": "method not allowed"})),
    }
}

fn setup_status(ctx: &Ctx) -> Value {
    let dir = &ctx.config_dir;
    let name = ctx.instance.as_deref().unwrap_or_default();
    json!({
        "agent": "baft-tunnel",
        "version": env!("CARGO_PKG_VERSION"),
        "setup_pending": true,
        "instance": name,
        "config_dir": dir.to_string_lossy(),
        "listener_accepting": false,
        "pairing": pairing_view(ctx),
        "setup_helper": match ctx.setup.available(name) {
            Ok(()) => json!({"available": true}),
            Err(why) => json!({"available": false, "reason": why}),
        },
        "pki_helper": if !ctx.pki.supports_config_dir(dir) {
            json!({"available": false, "reason": "the root PKI helper only serves /etc/baft-tunnel"})
        } else {
            match ctx.pki.available(name) {
                Ok(()) => json!({"available": true}),
                Err(why) => json!({"available": false, "reason": why}),
            }
        },
        "tunnel": null,
        "warnings": [],
    })
}

/// What exists of the local pairing (public files only): init done, own bundle present.
fn pairing_view(ctx: &Ctx) -> Value {
    let dir = &ctx.config_dir;
    let state: Option<Value> = std::fs::read(dir.join(crate::setup::STATE_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let own = read_bundle(&dir.join("bundle.json")).ok();
    json!({
        "initialized": state.is_some(),
        "tunnel_id": state.as_ref().map(|s| s["tunnel_id"].clone()),
        "node_id": state.as_ref().map(|s| s["node_id"].clone()),
        "peer_id": state.as_ref().map(|s| s["peer_id"].clone()),
        "role": state.as_ref().map(|s| s["role"].clone()),
        "engine": state.as_ref().map(|s| s["engine"].clone()),
        "own_bundle": own.is_some(),
        "own_checksum": own.as_ref().map(|(_, b)| b.checksum()),
        "joined": ctx.instance_config().is_some_and(|p| p.is_file()),
    })
}

fn read_bundle(path: &Path) -> Result<(String, crate::setup::Bundle)> {
    let meta = std::fs::symlink_metadata(path).context("no bundle")?;
    if !meta.is_file() || meta.len() > crate::setup::BUNDLE_MAX_BYTES {
        bail!("the bundle is not a regular file of at most 4 KiB");
    }
    let text = std::fs::read_to_string(path)?;
    let bundle: crate::setup::Bundle =
        serde_json::from_str(&text).context("the bundle is not valid")?;
    Ok((text, bundle))
}

/// Export: this node's public pairing bundle (written by `init`) with its checksum. Public data
/// only (ADR-0026): no key, credential or host address is in a bundle.
fn own_bundle(ctx: &Ctx) -> (u16, Value) {
    match read_bundle(&ctx.config_dir.join("bundle.json")) {
        Ok((text, bundle)) => (
            200,
            json!({"bundle": text, "checksum": bundle.checksum(), "fields": bundle}),
        ),
        Err(e) => (
            404,
            json!({"error": "no public bundle yet; run the init stage first", "detail": format!("{e:#}")}),
        ),
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectBody {
    bundle: String,
    checksum: Option<String>,
}

/// Import preview: parse a peer bundle, compute its checksum, compare it with the value the
/// operator typed (out of band) and with this node's pairing state. Changes nothing.
fn inspect_bundle(ctx: &Ctx, body: &[u8]) -> (u16, Value) {
    let req: InspectBody = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(error) => {
            return (
                422,
                json!({"error": "invalid request", "detail": error.to_string()}),
            );
        }
    };
    if req.bundle.len() as u64 > crate::setup::BUNDLE_MAX_BYTES {
        return (422, json!({"error": "bundle is larger than 4 KiB"}));
    }
    let bundle: crate::setup::Bundle = match serde_json::from_str(&req.bundle) {
        Ok(b) => b,
        Err(error) => {
            return (
                422,
                json!({"error": "this is not a pairing bundle", "detail": error.to_string()}),
            );
        }
    };
    let checksum = bundle.checksum();
    let checksum_match = req
        .checksum
        .as_deref()
        .map(|c| crate::setup::same_checksum(c, &checksum));
    let state: Option<Value> = std::fs::read(ctx.config_dir.join(crate::setup::STATE_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let mut problems: Vec<&str> = Vec::new();
    if let Some(s) = &state {
        let peer_role = serde_json::to_value(bundle.role).unwrap_or_default();
        for (ok, what) in [
            (s["engine"] == json!(bundle.engine), "engine"),
            (s["direction"] == json!(bundle.direction), "direction"),
            (s["tunnel_id"] == json!(bundle.tunnel_id), "tunnel_id"),
            (
                s["service_port"] == json!(bundle.service_port),
                "service_port",
            ),
            (s["tunnel_port"] == json!(bundle.tunnel_port), "tunnel_port"),
            (s["peer_id"] == json!(bundle.node_id), "node_id"),
            (s["node_id"] == json!(bundle.peer_id), "peer_id"),
            (s["role"] != peer_role, "role"),
            (s["own_public"] != json!(bundle.public), "public"),
        ] {
            if !ok {
                problems.push(what);
            }
        }
    }
    (
        200,
        json!({
            "fields": bundle,
            "checksum": checksum,
            "checksum_match": checksum_match,
            "matches_this_node": state.as_ref().map(|_| problems.is_empty()),
            "mismatches": problems,
            "changed": false,
        }),
    )
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlanBody {
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
    pub(crate) fn to_request(&self) -> crate::setup::PlanRequest {
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

fn check_code(id: &str, result: &str, evidence: String, code: String, params: Value) -> Value {
    json!({"id": id, "result": result, "evidence": evidence, "code": code, "params": params})
}

fn check_with_params(id: &str, result: &str, evidence: String, params: Value) -> Value {
    check_code(id, result, evidence, format!("pf.{id}.{result}"), params)
}

fn check(id: &str, result: &str, evidence: String) -> Value {
    check_with_params(id, result, evidence, json!({}))
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
async fn preflight(ctx: &Ctx, body: &[u8]) -> (u16, Value) {
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
            checks.push(check_with_params(
                "direction_support",
                "pass",
                format!(
                    "{} supports the {direction} direction in this build",
                    req.engine.as_str()
                ),
                json!({"engine": req.engine.as_str(), "direction": direction}),
            ));
        } else {
            checks.push(check_with_params(
                "direction_support",
                "blocked",
                format!("the {direction} direction is not operational for {} in this build (no real data path has been proven for it)", req.engine.as_str()),
                json!({"engine": req.engine.as_str(), "direction": direction}),
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
    if ctx.via_helper(&req) {
        // Installed instance: root writes the files through the setup helper (ADR-0026).
        let name = req.instance.as_deref().unwrap_or_default();
        checks.push(match ctx.setup.available(name) {
            Ok(()) => check_with_params(
                "setup_helper",
                "pass",
                format!("the root setup helper for {name} is installed; it writes the files with the service owners"),
                json!({"instance": name}),
            ),
            Err(why) => check_with_params("setup_helper", "blocked", why, json!({"instance": name})),
        });
    } else {
        checks.push(directory_writable_check(dir));
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
    if req.engine == crate::config::EngineKind::TcpTls && ctx.via_helper(&req) {
        let name = req.instance.as_deref().unwrap_or_default();
        checks.push(if !ctx.pki.supports_config_dir(dir) {
            check(
                "pki_helper",
                "blocked",
                "the root PKI helper only serves /etc/baft-tunnel; custom config directories are not supported".into(),
            )
        } else {
            match ctx.pki.available(name) {
                Ok(()) => check_with_params(
                    "pki_helper",
                    "pass",
                    "the root PKI CSR helper is installed for this instance".into(),
                    json!({"instance": name}),
                ),
                Err(why) => check_with_params("pki_helper", "blocked", why, json!({"instance": name})),
            }
        });
    }
    if req.engine == crate::config::EngineKind::TcpTls {
        match crate::pki_helper::active_set_dir(dir) {
            Err(error) => checks.push(check(
                "pki_files",
                "blocked",
                format!("active credential set is invalid: {error:#}"),
            )),
            Ok(active_set) => {
                let material_dir = active_set.as_deref().unwrap_or(dir);
                let mut missing: Vec<&str> = ["node.key"]
                    .into_iter()
                    .filter(|name| !dir.join(name).is_file())
                    .collect();
                for name in ["node.crt", "ca.crt", "ca.crl"] {
                    if !material_dir.join(name).is_file() {
                        missing.push(name);
                    }
                }
                if missing.is_empty() {
                    checks.push(check("pki_files", "pass", "certificate, key, CA and CRL files are present (validity is checked by setup init)".into()));
                } else {
                    let no_certificate_material = ["node.crt", "ca.crt", "ca.crl"]
                        .into_iter()
                        .all(|name| !dir.join(name).exists() && !material_dir.join(name).exists());
                    let pristine = no_certificate_material
                        && !dir.join("node.key").exists()
                        && !dir.join("node.csr").exists()
                        && !dir.join("node.csr.meta").exists();
                    let csr_pending = no_certificate_material
                        && ["node.key", "node.csr", "node.csr.meta"]
                            .into_iter()
                            .all(|name| dir.join(name).is_file());
                    if pristine || csr_pending {
                        checks.push(check(
                            "pki_files",
                            "warning",
                            "certificate set is not installed yet; create or recover this node's CSR and import the signed certificate, CA and CRL in the panel before Init".into(),
                        ));
                    } else {
                        checks.push(check_with_params(
                            "pki_files",
                            "blocked",
                            format!(
                                "partial or incomplete credentials; missing: {}",
                                missing.join(", ")
                            ),
                            json!({"missing": missing.join(", ")}),
                        ));
                    }
                }
            }
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
        (true, true) => checks.push(check_code("service_port", "pass", format!("something accepts connections on 127.0.0.1:{}", req.service_port), "pf.service_port.serve_accepting".into(), json!({"port": req.service_port}))),
        (true, false) => checks.push(check_code("service_port", "warning", format!("nothing accepts connections on 127.0.0.1:{} yet; start the service before users connect", req.service_port), "pf.service_port.serve_waiting".into(), json!({"port": req.service_port}))),
        (false, true) => checks.push(check_code("service_port", "warning", format!("127.0.0.1:{} is already in use on this host", req.service_port), "pf.service_port.forward_busy".into(), json!({"port": req.service_port}))),
        (false, false) => checks.push(check_code("service_port", "pass", format!("127.0.0.1:{} is free", req.service_port), "pf.service_port.forward_free".into(), json!({"port": req.service_port}))),
    }
    if serve {
        if port_accepting(req.tunnel_port).await {
            checks.push(check_with_params(
                "tunnel_port",
                "warning",
                format!(
                    "port {} already accepts connections here (possibly a running tunnel)",
                    req.tunnel_port
                ),
                json!({"port": req.tunnel_port}),
            ));
        } else {
            checks.push(check_with_params(
                "tunnel_port",
                "pass",
                format!("port {} is not in use on loopback", req.tunnel_port),
                json!({"port": req.tunnel_port}),
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
        return check_with_params(
            "peer_reachability",
            "unknown",
            "the peer address is not an IP address; not probed".into(),
            json!({"port": port}),
        );
    };
    match timeout(
        PEER_PROBE_TIMEOUT,
        TcpStream::connect(SocketAddr::new(ip, port)),
    )
    .await
    {
        Ok(Ok(_)) => check_with_params(
            "peer_reachability",
            "pass",
            format!(
                "a TCP connection to the peer on port {port} was accepted (who answered is not verified)"
            ),
            json!({"port": port}),
        ),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => check_with_params(
            "peer_reachability",
            "warning",
            format!("the peer host answered but nothing listens on port {port} yet"),
            json!({"port": port}),
        ),
        Ok(Err(e)) => check_code(
            "peer_reachability",
            "blocked",
            format!("cannot reach the peer on port {port}: {e}"),
            "pf.peer_reachability.error".into(),
            json!({"port": port}),
        ),
        Err(_) => check_with_params(
            "peer_reachability",
            "blocked",
            format!(
                "no answer from the peer on port {port} within {} s",
                PEER_PROBE_TIMEOUT.as_secs()
            ),
            json!({"port": port, "timeout_seconds": PEER_PROBE_TIMEOUT.as_secs()}),
        ),
    }
}

/// Can this agent process create files where Apply `init` will write them? On the installed
/// layout `/etc/baft-tunnel` is `root:baft-tunnel 0750`, so an agent running as `baft-tunnel`
/// cannot; Apply must then stay disabled and the printed commands are run as root instead.
/// Read-only: uses access(2), never creates a probe file.
fn directory_writable_check(dir: &std::path::Path) -> Value {
    let target = if dir.is_dir() {
        Some(dir)
    } else if dir.exists() {
        None
    } else {
        dir.parent().filter(|p| p.is_dir())
    };
    let Some(target) = target else {
        return check_with_params(
            "directory_writable",
            "unknown",
            "neither the setup directory nor its parent exists, so write access could not be checked".into(),
            json!({"path": dir.display().to_string()}),
        );
    };
    let Ok(c_path) = std::ffi::CString::new(target.as_os_str().as_encoded_bytes()) else {
        return check_with_params(
            "directory_writable",
            "blocked",
            "the setup path contains a NUL byte".into(),
            json!({"path": dir.display().to_string()}),
        );
    };
    // SAFETY: c_path is a valid NUL-terminated string; access(2) only reads it.
    let ok = unsafe { libc::access(c_path.as_ptr(), libc::W_OK | libc::X_OK) } == 0;
    if ok {
        check_with_params(
            "directory_writable",
            "pass",
            format!("the agent can create files in {}", target.display()),
            json!({"path": target.display().to_string()}),
        )
    } else {
        check_with_params(
            "directory_writable",
            "blocked",
            format!(
                "the agent user cannot create files in {}; Apply cannot run here. Run the printed setup commands as root instead",
                target.display()
            ),
            json!({"path": target.display().to_string()}),
        )
    }
}

fn remediation(id: &str) -> &'static str {
    match id {
        "planner" => "Fix the highlighted setting; the same rule applies on the command line.",
        "direction_support" => {
            "Choose the Direct direction. Reverse is unavailable until its implementation passes independent acceptance."
        }
        "directory" => "Create the setup directory or choose another one.",
        "setup_helper" => {
            "Run sudo scripts/install-agent-control.sh --instance NAME (or install.sh --register NAME) to install the root setup helper."
        }
        "pki_helper" => {
            "Run sudo scripts/install-agent-control.sh --instance NAME to install the root PKI CSR helper."
        }
        "directory_writable" => {
            "Run the printed setup commands as root on this server; the panel Apply button stays disabled here."
        }
        "existing_pairing" => {
            "Choose another directory or instance; setup never overwrites a pairing."
        }
        "pki_files" => {
            "Complete the panel TLS certificate step: create or recover the CSR, obtain an offline CA signature, and import the node certificate, CA and CRL before Init."
        }
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
            if c.get("code").is_none() {
                c["code"] = json!(format!("pf.{id}.{result}"));
            }
            if c.get("params").is_none() {
                c["params"] = json!({});
            }
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
        let (code, report) = preflight(ctx, &plan_bytes).await;
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
    if let Some(name) = &ctx.instance {
        if !request.stage.is_lifecycle() && !ctx.via_helper(&request.plan) {
            return (
                422,
                json!({"error": format!("this agent sets up only instance {name} in {}", ctx.config_dir.display())}),
            );
        }
    }
    let blocked = gate.as_ref().is_some_and(|g| !g.blocked.is_empty());
    if !request.stage.is_lifecycle() && ctx.via_helper(&request.plan) && !blocked {
        let (code, receipt) = match helper_apply(ctx, &request, &receipts_dir).await {
            Ok(out) => out,
            Err((code, error)) => return (code, json!({"error": error})),
        };
        if receipt["replayed"] != json!(true) {
            let stage = request.stage.as_str();
            let _ = audit(
                state_dir,
                ctx,
                AuditEvent {
                    action: "apply",
                    result: receipt["result"].as_str().unwrap_or("conflict"),
                    resource: format!("setup/{stage}"),
                    node: Some(&request.plan.node_id),
                    instance: request.plan.instance.as_deref(),
                    sha256: receipt["request_sha256"].as_str().unwrap_or_default(),
                    detail: stage,
                    receipt_id: receipt["id"].as_str(),
                },
            );
        }
        return (code, receipt);
    }
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
        let stage = request.stage.as_str();
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

/// `init`/`join` of the bound instance through the root setup helper (ADR-0026). The agent keeps
/// its own copy of the receipt (idempotent replay) and accepts the helper's answer only when it
/// carries this request's key and digest.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PkiCertificateBody {
    request_id: String,
    node_id: String,
    server_name: String,
    confirm: bool,
    certificate_pem: String,
    ca_pem: String,
    crl_pem: String,
}

/// Installs only a fully validated public credential set. The root helper stages an immutable
/// version and atomically switches one symlink; this endpoint never restarts or applies a tunnel.
async fn pki_certificate_action(ctx: &Ctx, body: &[u8]) -> (u16, Value) {
    const MAX_CERTIFICATE_BODY_BYTES: usize = crate::pki_helper::MAX_REQUEST_BYTES as usize;
    if body.len() > MAX_CERTIFICATE_BODY_BYTES {
        return (
            413,
            json!({"error": "certificate upload exceeds the bounded request size"}),
        );
    }
    let Some(state_dir) = ctx.state_dir.as_ref() else {
        return (
            404,
            json!({"error": "PKI operations are disabled; audit state is unavailable"}),
        );
    };
    let Some(name) = ctx.instance.as_deref() else {
        return (
            409,
            json!({"error": "PKI enrollment requires an instance-bound agent"}),
        );
    };
    if !crate::apply::valid_instance(name) || !ctx.pki.supports_config_dir(&ctx.config_dir) {
        return (
            409,
            json!({"error": "PKI helper is not bound to the fixed system config directory"}),
        );
    }
    let req: PkiCertificateBody = match serde_json::from_slice(body) {
        Ok(req) => req,
        Err(error) => {
            return (
                422,
                json!({"error": "invalid certificate upload request", "detail": error.to_string()}),
            );
        }
    };
    if !req.confirm {
        return (
            409,
            json!({"error": "explicit certificate installation confirmation is required"}),
        );
    }
    if !crate::apply::valid_key(&req.request_id)
        || !crate::pki_helper::valid_node_id(&req.node_id)
        || crate::pki_helper::expected_server_san(&req.server_name).is_err()
    {
        return (
            422,
            json!({"error": "request ID, node identity or DNS/IP server name is invalid"}),
        );
    }
    if req.certificate_pem.is_empty()
        || req.certificate_pem.len() > crate::pki_helper::MAX_CERTIFICATE_BYTES
        || req.ca_pem.is_empty()
        || req.ca_pem.len() > crate::pki_helper::MAX_CA_BYTES
        || req.crl_pem.is_empty()
        || req.crl_pem.len() > crate::pki_helper::MAX_CRL_BYTES
    {
        return (
            413,
            json!({"error": "certificate, CA or CRL PEM is empty or exceeds its individual size limit"}),
        );
    }
    let request = crate::pki_helper::certificate_request_document(
        &req.request_id,
        &req.node_id,
        &req.server_name,
        &req.certificate_pem,
        &req.ca_pem,
        &req.crl_pem,
    );
    let request_bytes = match serde_json::to_vec(&request) {
        Ok(bytes) if bytes.len() as u64 <= crate::pki_helper::MAX_REQUEST_BYTES => bytes,
        _ => {
            return (
                413,
                json!({"error": "encoded certificate upload exceeds the root helper request limit"}),
            );
        }
    };
    let digest = hex::encode(Sha256::digest(&request_bytes));
    let operation = crate::pki_helper::PkiOperation::InstallCertificate;
    // Serialize credential installation with other state-changing agent operations, but
    // keep the blocking helper wait off the async runtime worker (the helper may take 65 s).
    let _guard = ctx.lock.lock().await;
    let helper = ctx.pki.clone();
    let instance = name.to_owned();
    let receipt = match tokio::task::spawn_blocking(move || helper.submit(&instance, &request))
        .await
    {
        Ok(Ok(receipt)) => receipt,
        Ok(Err(error)) => {
            audit_pki_csr(
                state_dir,
                ctx,
                name,
                &req.node_id,
                operation,
                &digest,
                &req.request_id,
                "helper_error",
            );
            return (
                503,
                json!({"error": "root PKI helper did not complete certificate installation", "detail": error}),
            );
        }
        Err(error) => {
            audit_pki_csr(
                state_dir,
                ctx,
                name,
                &req.node_id,
                operation,
                &digest,
                &req.request_id,
                "helper_task_error",
            );
            return (
                500,
                json!({"error": format!("PKI helper task failed: {error}")}),
            );
        }
    };
    let result = receipt["result"].as_str().unwrap_or_default();
    if result != "installed" {
        audit_pki_csr(
            state_dir,
            ctx,
            name,
            &req.node_id,
            operation,
            &digest,
            &req.request_id,
            "refused",
        );
        return (
            409,
            json!({"error": "certificate set was refused; the active credential set was not changed", "detail": receipt["reason"], "receipt": receipt}),
        );
    }
    let set_id = receipt["active_set"].as_str().unwrap_or_default();
    let fingerprint = receipt["certificate_sha256"].as_str().unwrap_or_default();
    let not_after = receipt["certificate_not_after"]
        .as_i64()
        .unwrap_or_default();
    let crl_remaining = receipt["crl_seconds_remaining"]
        .as_i64()
        .unwrap_or_default();
    if receipt["receipt_version"] != json!(1)
        || receipt["request_id"] != json!(req.request_id)
        || receipt["operation"] != json!("install_certificate")
        || receipt["node_id"] != json!(req.node_id)
        || receipt["private_key_exported"] != json!(false)
        || set_id.len() != 64
        || !set_id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || fingerprint.len() != 64
        || !fingerprint
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || not_after <= 0
        || crl_remaining < 24 * 3600
    {
        audit_pki_csr(
            state_dir,
            ctx,
            name,
            &req.node_id,
            operation,
            &digest,
            &req.request_id,
            "invalid_receipt",
        );
        return (
            502,
            json!({"error": "root PKI helper returned an invalid certificate installation receipt"}),
        );
    }
    audit_pki_csr(
        state_dir,
        ctx,
        name,
        &req.node_id,
        operation,
        &digest,
        &req.request_id,
        "installed",
    );
    (
        200,
        json!({
            "result": "installed",
            "node_id": req.node_id,
            "certificate_sha256": fingerprint,
            "certificate_not_after": not_after,
            "crl_seconds_remaining": crl_remaining,
            "active_set": set_id,
            "service_restarted": false,
            "private_key_exported": false
        }),
    )
}

async fn helper_apply(
    ctx: &Ctx,
    request: &crate::apply::ApplyRequest,
    receipts: &Path,
) -> std::result::Result<(u16, Value), (u16, String)> {
    let digest = crate::apply::request_digest(request);
    if let Some(old) = crate::apply::replay_key(receipts, &request.key, &digest) {
        return Ok(old);
    }
    let name = ctx.instance.clone().unwrap_or_default();
    let document = crate::setup_helper::request_document(request);
    let helper = ctx.setup.clone();
    let mut receipt = tokio::task::spawn_blocking(move || helper.submit(&name, &document))
        .await
        .map_err(|e| (500, format!("setup helper task failed: {e}")))?
        .map_err(|e| (503, e))?;
    if receipt["id"] != json!(request.key) || receipt["request_sha256"] != json!(digest) {
        let reason = receipt["reason"].as_str().unwrap_or("").to_string();
        return Err((
            502,
            format!(
                "the setup helper answered for another request; nothing is trusted from it. {reason}"
            ),
        ));
    }
    receipt["replayed"] = json!(false);
    crate::apply::store_receipt(receipts, &request.key, &mut receipt);
    Ok((crate::apply::result_code(&receipt), receipt))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PkiCsrBody {
    request_id: String,
    node_id: String,
    server_name: String,
    #[serde(default)]
    confirm: bool,
}

#[allow(clippy::too_many_arguments)]
fn audit_pki_csr(
    state_dir: &Path,
    ctx: &Ctx,
    instance: &str,
    node_id: &str,
    operation: crate::pki_helper::PkiOperation,
    digest: &str,
    request_id: &str,
    result: &str,
) {
    let _ = audit(
        state_dir,
        ctx,
        AuditEvent {
            action: "pki_csr",
            result,
            resource: "pki/csr".into(),
            node: Some(node_id),
            instance: Some(instance),
            sha256: digest,
            detail: operation.as_str(),
            receipt_id: Some(request_id),
        },
    );
}

/// Pre-Init CSR generation and recovery. The Agent never reads node.key; it only submits a
/// bounded public request to the fixed root PKI helper and validates the public receipt.
async fn pki_csr_action(
    ctx: &Ctx,
    body: &[u8],
    operation: crate::pki_helper::PkiOperation,
) -> (u16, Value) {
    const MAX_PKI_BODY_BYTES: usize = 4096;
    if body.len() > MAX_PKI_BODY_BYTES {
        return (413, json!({"error": "PKI request exceeds 4 KiB"}));
    }
    let Some(state_dir) = ctx.state_dir.as_ref() else {
        return (
            404,
            json!({"error": "PKI operations are disabled; audit state is unavailable"}),
        );
    };
    let Some(name) = ctx.instance.as_deref() else {
        return (
            409,
            json!({"error": "PKI enrollment requires an instance-bound agent"}),
        );
    };
    if !crate::apply::valid_instance(name) {
        return (409, json!({"error": "the bound instance name is invalid"}));
    }
    if !ctx.pki.supports_config_dir(&ctx.config_dir) {
        return (
            409,
            json!({"error": "the root PKI helper only serves /etc/baft-tunnel; custom config directories are not supported"}),
        );
    }
    let req: PkiCsrBody = match serde_json::from_slice(body) {
        Ok(req) => req,
        Err(error) => {
            return (
                422,
                json!({"error": "invalid PKI request", "detail": error.to_string()}),
            );
        }
    };
    if !crate::apply::valid_key(&req.request_id) {
        return (
            422,
            json!({"error": "request_id must be 8-64 characters of A-Z a-z 0-9 - _"}),
        );
    }
    if !crate::pki_helper::valid_node_id(&req.node_id) {
        return (422, json!({"error": "node_id is invalid"}));
    }
    let expected_san = match crate::pki_helper::expected_server_san(&req.server_name) {
        Ok(value) => value,
        Err(error) => {
            return (
                422,
                json!({"error": "server_name is invalid", "detail": format!("{error:#}")}),
            );
        }
    };
    if operation == crate::pki_helper::PkiOperation::CreateCsr && !req.confirm {
        return (
            422,
            json!({"error": "CSR generation requires confirm: true"}),
        );
    }

    // CSR generation is strictly before Init. Once a config exists, only read is available.
    if let Some(config) = ctx.config.as_ref() {
        let (local_id, engine) = match config {
            RuntimeConfig::Serve {
                local_id, engine, ..
            } => (local_id.as_str(), *engine),
            RuntimeConfig::Forward {
                local_id, engine, ..
            } => (local_id.as_str(), *engine),
        };
        if operation == crate::pki_helper::PkiOperation::CreateCsr {
            return (
                409,
                json!({"error": "CSR generation is only available before Init"}),
            );
        }
        if engine != crate::config::EngineKind::TcpTls || local_id != req.node_id {
            return (
                409,
                json!({"error": "CSR retrieval is only available for this configured tcp-tls node"}),
            );
        }
    } else {
        if operation == crate::pki_helper::PkiOperation::CreateCsr
            && ctx.instance_config().is_some_and(|path| path.is_file())
        {
            return (
                409,
                json!({"error": "an instance config already exists; CSR generation is pre-Init only"}),
            );
        }
        let state_path = ctx.config_dir.join(crate::setup::STATE_FILE);
        let state = match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&state_path)
        {
            Ok(mut file) => {
                let meta = match file.metadata() {
                    Ok(meta) => meta,
                    Err(error) => {
                        return (
                            409,
                            json!({"error": "cannot inspect existing setup state", "detail": error.to_string()}),
                        );
                    }
                };
                if !meta.is_file() || meta.len() > 16 * 1024 {
                    return (
                        409,
                        json!({"error": "existing setup state is not a safe bounded regular file"}),
                    );
                }
                let mut bytes = Vec::new();
                if let Err(error) = std::io::Read::by_ref(&mut file)
                    .take(16 * 1024 + 1)
                    .read_to_end(&mut bytes)
                {
                    return (
                        409,
                        json!({"error": "cannot read existing setup state", "detail": error.to_string()}),
                    );
                }
                if bytes.len() > 16 * 1024 {
                    return (409, json!({"error": "existing setup state exceeds 16 KiB"}));
                }
                match serde_json::from_slice::<Value>(&bytes) {
                    Ok(state) => Some(state),
                    Err(error) => {
                        return (
                            409,
                            json!({"error": "existing setup state is invalid", "detail": error.to_string()}),
                        );
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return (
                    409,
                    json!({"error": "cannot safely open existing setup state", "detail": error.to_string()}),
                );
            }
        };
        if let Some(state) = state {
            if state["engine"].as_str() != Some("tcp-tls") {
                return (
                    409,
                    json!({"error": "PKI enrollment is only valid for tcp-tls"}),
                );
            }
            if state["node_id"].as_str() != Some(req.node_id.as_str()) {
                return (
                    409,
                    json!({"error": "node_id does not match the existing setup state"}),
                );
            }
        }
    }

    let request = crate::pki_helper::request_document(
        &req.request_id,
        operation,
        &req.node_id,
        &req.server_name,
    );
    let request_bytes = match serde_json::to_vec(&request) {
        Ok(bytes) => bytes,
        Err(error) => {
            return (
                500,
                json!({"error": format!("cannot encode PKI request: {error}")}),
            );
        }
    };
    let digest = hex::encode(Sha256::digest(&request_bytes));
    let _guard = ctx.lock.lock().await;
    if let Err(error) = ctx.pki.available(name) {
        audit_pki_csr(
            state_dir,
            ctx,
            name,
            &req.node_id,
            operation,
            &digest,
            &req.request_id,
            "helper_unavailable",
        );
        return (
            503,
            json!({"error": "root PKI helper unavailable", "detail": error}),
        );
    }
    let helper = ctx.pki.clone();
    let instance = name.to_owned();
    let receipt =
        match tokio::task::spawn_blocking(move || helper.submit(&instance, &request)).await {
            Ok(Ok(receipt)) => receipt,
            Ok(Err(error)) => {
                audit_pki_csr(
                    state_dir,
                    ctx,
                    name,
                    &req.node_id,
                    operation,
                    &digest,
                    &req.request_id,
                    "helper_error",
                );
                return (
                    503,
                    json!({"error": "root PKI helper failed", "detail": error}),
                );
            }
            Err(error) => {
                audit_pki_csr(
                    state_dir,
                    ctx,
                    name,
                    &req.node_id,
                    operation,
                    &digest,
                    &req.request_id,
                    "helper_task_error",
                );
                return (
                    500,
                    json!({"error": format!("PKI helper task failed: {error}")}),
                );
            }
        };

    let expected_operation = operation.as_str();
    if receipt["request_id"] != json!(req.request_id)
        || receipt["operation"] != json!(expected_operation)
        || receipt["private_key_exported"] != json!(false)
    {
        audit_pki_csr(
            state_dir,
            ctx,
            name,
            &req.node_id,
            operation,
            &digest,
            &req.request_id,
            "invalid_receipt",
        );
        return (
            502,
            json!({"error": "PKI helper receipt did not match this request or violated key-custody policy"}),
        );
    }
    let result = receipt["result"].as_str().unwrap_or("refused");
    if result == "refused" {
        audit_pki_csr(
            state_dir,
            ctx,
            name,
            &req.node_id,
            operation,
            &digest,
            &req.request_id,
            "refused",
        );
        return (
            409,
            json!({"error": "CSR operation refused", "detail": receipt["reason"], "receipt": receipt}),
        );
    }
    let expected_result = match operation {
        crate::pki_helper::PkiOperation::CreateCsr => "created",
        crate::pki_helper::PkiOperation::ReadCsr => "retrieved",
        crate::pki_helper::PkiOperation::InstallCertificate => "installed",
    };
    let csr = receipt["csr_pem"].as_str().unwrap_or_default();
    let fingerprint = receipt["public_key_sha256"].as_str().unwrap_or_default();
    if result != expected_result
        || receipt["node_id"] != json!(req.node_id)
        || receipt["expected_server_san"] != json!(expected_san)
        || csr.len() > crate::pki_helper::MAX_CSR_BYTES as usize
        || !csr.starts_with("-----BEGIN CERTIFICATE REQUEST-----")
        || !csr
            .trim_end()
            .ends_with("-----END CERTIFICATE REQUEST-----")
        || fingerprint.len() != 64
        || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        audit_pki_csr(
            state_dir,
            ctx,
            name,
            &req.node_id,
            operation,
            &digest,
            &req.request_id,
            "invalid_receipt",
        );
        return (
            502,
            json!({"error": "PKI helper returned an invalid CSR receipt"}),
        );
    }
    let replayed = receipt["replayed"] == json!(true);
    audit_pki_csr(
        state_dir,
        ctx,
        name,
        &req.node_id,
        operation,
        &digest,
        &req.request_id,
        if replayed { "replayed" } else { result },
    );
    let code = if operation == crate::pki_helper::PkiOperation::CreateCsr && !replayed {
        201
    } else {
        200
    };
    (code, receipt)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleBody {
    /// `start` (and enable at boot), `stop` (and disable at boot) or `restart`.
    action: crate::apply::Stage,
    /// Must be literally `true`: this changes the running tunnel.
    confirm: bool,
    idempotency_key: String,
}

/// Panel Start / Stop / Restart for the instance this agent is bound to (`--instance`). The agent
/// only drops a request file for the root control helper; it has no other systemd rights.
async fn tunnel_lifecycle(ctx: &Ctx, body: &[u8]) -> (u16, Value) {
    let Some(state_dir) = &ctx.state_dir else {
        return (
            404,
            json!({"error": "lifecycle actions are disabled; receipts need the state directory"}),
        );
    };
    let Some(name) = ctx.instance.clone() else {
        return (
            409,
            json!({"error": "the agent was started without --instance; it does not manage a unit"}),
        );
    };
    let req: LifecycleBody = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(error) => {
            return (
                422,
                json!({"error": "invalid request", "detail": error.to_string()}),
            );
        }
    };
    if !req.confirm {
        return (
            422,
            json!({"error": "lifecycle actions require \"confirm\": true"}),
        );
    }
    if !req.action.is_lifecycle() {
        return (
            422,
            json!({"error": "action must be start, stop or restart"}),
        );
    }
    if !crate::apply::valid_key(&req.idempotency_key) {
        return (
            422,
            json!({"error": "idempotency_key must be 8-64 characters of A-Z a-z 0-9 - _"}),
        );
    }
    let _guard = ctx.lock.lock().await;
    let receipts_dir = state_dir.join("receipts");
    let (units, dir, stage, key) = (
        ctx.units.clone(),
        ctx.config_dir.clone(),
        req.action,
        req.idempotency_key.clone(),
    );
    let instance = name.clone();
    let (code, receipt) = match tokio::task::spawn_blocking(move || {
        crate::apply::lifecycle(stage, &instance, &dir, &key, &receipts_dir, units.as_ref())
    })
    .await
    {
        Ok(out) => out,
        Err(e) => return (500, json!({"error": format!("lifecycle task failed: {e}")})),
    };
    if receipt["replayed"] != json!(true) && receipt.get("receipt_version").is_some() {
        let digest = receipt["request_sha256"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let _ = audit(
            state_dir,
            ctx,
            AuditEvent {
                action: "lifecycle",
                result: receipt["result"].as_str().unwrap_or("conflict"),
                resource: format!("tunnel/{}", stage.as_str()),
                node: None,
                instance: Some(&name),
                sha256: &digest,
                detail: stage.as_str(),
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
    let crl_remaining = crate::tls::crl_seconds_remaining(crl).ok();
    json!({
        "engine": config.engine_name(),
        "tls": true,
        "crl_seconds_remaining": crl_remaining,
        "crl_state": crl_state(crl_remaining),
        "certificate_seconds_remaining": crate::tls::certificate_seconds_remaining(cert).ok(),
    })
}

/// `expired`: peers refuse every connection now. `renew_now`: under 72 h, the point where
/// `push-crl.sh` refuses to distribute it. `renew_soon`: under 7 days. `unknown`: unreadable.
fn crl_state(remaining: Option<i64>) -> &'static str {
    match remaining {
        None => "unknown",
        Some(r) if r <= 0 => "expired",
        Some(r) if r < 72 * 3600 => "renew_now",
        Some(r) if r < crate::config::CRL_WARNING_SECONDS => "renew_soon",
        Some(_) => "ok",
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IntegrityVerifyBody {
    idempotency_key: String,
}

const INTEGRITY_RECEIPTS_FILE: &str = "integrity-probe-receipts.json";
const INTEGRITY_RECEIPTS_MAX: usize = 64;

fn integrity_request_hash(key: &str, config: &RuntimeConfig) -> String {
    let material = json!({
        "operation": "integrity_probe",
        "idempotency_key": key,
        "node_id": config.ids().0,
        "peer_id": config.ids().1,
        "tunnel_id": config.ids().2,
        "direction": config.direction_name(),
    });
    hex::encode(Sha256::digest(material.to_string().as_bytes()))
}

fn load_integrity_receipts(dir: &Path) -> Result<Value> {
    match std::fs::read(dir.join(INTEGRITY_RECEIPTS_FILE)) {
        Ok(bytes) => {
            let store: Value = serde_json::from_slice(&bytes)?;
            if store["version"] != 1 || !store["entries"].is_array() {
                bail!("integrity receipt store has an unsupported schema");
            }
            Ok(store)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(json!({"version": 1, "entries": []}))
        }
        Err(error) => Err(error.into()),
    }
}

fn save_integrity_receipts(dir: &Path, store: &Value) -> Result<()> {
    write_private(dir, INTEGRITY_RECEIPTS_FILE, serde_json::to_string(store)?.as_bytes())
}

async fn verify_integrity(
    ctx: &Ctx,
    config: &RuntimeConfig,
    body: &[u8],
) -> (u16, Value) {
    let Some(state_dir) = ctx.state_dir.as_deref() else {
        return (409, json!({"result":"unknown","reason":"audit_unavailable"}));
    };
    let request: IntegrityVerifyBody = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(_) => return (422, json!({"error":"invalid request"})),
    };
    if !crate::apply::valid_key(&request.idempotency_key) {
        return (
            422,
            json!({"error":"idempotency_key must be 8-64 characters of A-Z a-z 0-9 - _"}),
        );
    }

    // Serialize probe operations so the durable receipt journal cannot double-start on retry.
    let _guard = ctx.lock.lock().await;
    let request_sha256 = integrity_request_hash(&request.idempotency_key, config);
    let mut receipts = match load_integrity_receipts(state_dir) {
        Ok(receipts) => receipts,
        Err(_) => return (500, json!({"result":"unknown","reason":"receipt_store_unavailable"})),
    };

    let existing_index = receipts["entries"]
        .as_array()
        .and_then(|entries| {
            entries.iter().position(|entry| {
                entry["key"].as_str() == Some(request.idempotency_key.as_str())
            })
        });
    if let Some(index) = existing_index {
        let entry = &receipts["entries"][index];
        if entry["request_sha256"].as_str() != Some(request_sha256.as_str()) {
            return (409, json!({"error":"idempotency_conflict"}));
        }
        if entry["state"] == "completed" && entry["response"].is_object() {
            let mut replay = entry["response"].clone();
            if let Some(fields) = replay.as_object_mut() {
                fields.insert("replayed".into(), json!(true));
            }
            let status = entry["http_status"].as_u64().unwrap_or(200) as u16;
            return (status, replay);
        }
        return (
            409,
            json!({
                "result": "unknown",
                "reason": "request_in_progress",
                "receipt_id": request.idempotency_key,
            }),
        );
    }

    {
        let entries = receipts["entries"].as_array_mut().expect("validated receipt entries");
        if entries.len() >= INTEGRITY_RECEIPTS_MAX {
            if let Some(index) = entries.iter().position(|entry| entry["state"] == "completed") {
                entries.remove(index);
            } else {
                return (503, json!({"result":"unknown","reason":"receipt_store_full"}));
            }
        }
        entries.push(json!({
            "key": request.idempotency_key.clone(),
            "request_sha256": request_sha256,
            "state": "in_progress",
            "created_unix": unix_now(),
            "response": null,
        }));
    }
    if save_integrity_receipts(state_dir, &receipts).is_err() {
        return (500, json!({"result":"unknown","reason":"receipt_store_unavailable"}));
    }

    let probe = match ctx.runtime_dir.as_deref() {
        Some(runtime_dir) => match crate::control_socket::request(&runtime_dir.join("control.sock")).await {
            Ok(response) => response,
            Err(_) => crate::control_socket::ProbeControlResponse {
                result: "unknown".into(),
                reason: Some("control_unavailable".into()),
                bytes: 0,
                sha256_match: false,
                duration_ms: None,
                timestamp: unix_now(),
            },
        },
        None => crate::control_socket::ProbeControlResponse {
            result: "unknown".into(),
            reason: Some("control_unavailable".into()),
            bytes: 0,
            sha256_match: false,
            duration_ms: None,
            timestamp: unix_now(),
        },
    };

    let resource = format!(
        "integrity_probe/tunnel/{}/peer/{}/direction/{}",
        config.ids().2,
        config.ids().1,
        config.direction_name()
    );
    let audit_result = audit(
        state_dir,
        ctx,
        AuditEvent {
            action: "integrity_probe",
            result: probe.result.as_str(),
            resource,
            node: Some(config.ids().0),
            instance: ctx.instance.as_deref(),
            sha256: &request_sha256,
            detail: probe.reason.as_deref().unwrap_or("sha256_verified"),
            receipt_id: Some(&request.idempotency_key),
        },
    );

    let mut response = serde_json::to_value(probe)
        .unwrap_or_else(|_| json!({"result":"unknown","reason":"response_encoding_failed"}));
    if let Some(fields) = response.as_object_mut() {
        fields.insert("peer_id".into(), json!(config.ids().1));
        fields.insert("tunnel_id".into(), json!(config.ids().2));
        fields.insert("direction".into(), json!(config.direction_name()));
        fields.insert("receipt_id".into(), json!(request.idempotency_key.clone()));
        fields.insert("replayed".into(), json!(false));
    }
    let status = if audit_result.is_err() {
        response = json!({
            "result":"unknown",
            "reason":"audit_failed",
            "bytes":0,
            "sha256_match":false,
            "timestamp":unix_now(),
            "peer_id":config.ids().1,
            "tunnel_id":config.ids().2,
            "direction":config.direction_name(),
            "receipt_id":request.idempotency_key.clone(),
            "replayed":false,
        });
        500
    } else {
        200
    };

    let entries = receipts["entries"].as_array_mut().expect("receipt entries");
    if let Some(entry) = entries.iter_mut().find(|entry| {
        entry["key"].as_str() == Some(request.idempotency_key.as_str())
    }) {
        entry["state"] = json!("completed");
        entry["completed_unix"] = json!(unix_now());
        entry["http_status"] = json!(status);
        entry["response"] = response.clone();
    }
    if save_integrity_receipts(state_dir, &receipts).is_err() {
        return (500, json!({"result":"unknown","reason":"receipt_store_unavailable"}));
    }
    (status, response)
}
fn get_audit(ctx: &Ctx) -> (u16, Value) {
    let Some(dir) = &ctx.state_dir else {
        return (404, json!({"error": "desired state is disabled"}));
    };
    // Newest-last, at most 100. Reads only the tail of each file (SA-08), so the cost is bounded.
    let mut entries: Vec<Value> = Vec::new();
    for name in [AUDIT_ROTATED, AUDIT_FILE] {
        entries.extend(
            audit_tail(&dir.join(name))
                .lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok()),
        );
    }
    let start = entries.len().saturating_sub(100);
    (200, json!({"entries": entries.split_off(start)}))
}

/// The last `AUDIT_TAIL_BYTES` of an audit file, cut to whole lines. Missing file: empty.
fn audit_tail(path: &Path) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(AUDIT_TAIL_BYTES);
    if f.seek(SeekFrom::Start(from)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if f.take(AUDIT_TAIL_BYTES).read_to_end(&mut buf).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    if from > 0 {
        // The first line may be cut in half: drop it.
        text.split_once('\n')
            .map(|(_, r)| r.to_string())
            .unwrap_or_default()
    } else {
        text
    }
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
    let path = dir.join(AUDIT_FILE);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= AUDIT_MAX_BYTES) {
        // Bounded growth: keep one previous generation (SA-08). Rename is atomic.
        std::fs::rename(&path, dir.join(AUDIT_ROTATED))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)?;
    let line = json!({
        "audit_version": 2,
        "ts": ts,
        "actor": ctx.actor(),
        "action": e.action,
        "target": {
            "node": e.node.or(ctx.config.as_ref().map(|c| c.ids().0)),
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

async fn status(ctx: &Ctx, config: &RuntimeConfig) -> Value {
    let listening = listener_accepting(config).await;
    json!({
        "agent": "baft-tunnel",
        "version": env!("CARGO_PKG_VERSION"),
        "engine": config.engine_name(),
        "config": config.support_report(),
        "listener_accepting": listening,
        "tunnel": tunnel_status(ctx, config, listening).await,
        "warnings": status_warnings(config),
    })
}

/// Operator warnings drawn only from local observation. Today: CRL lifetime, because an expired
/// CRL makes both TLS roles refuse every connection (fail closed).
fn status_warnings(config: &RuntimeConfig) -> Value {
    let Some((crl, _)) = config.tls_files() else {
        return json!([]);
    };
    let remaining = crate::tls::crl_seconds_remaining(crl).ok();
    let state = crl_state(remaining);
    let (severity, text) = match state {
        "ok" => return json!([]),
        "expired" => (
            "error",
            "the CRL has expired; peers refuse every connection until a renewed CRL is installed",
        ),
        "renew_now" => (
            "error",
            "the CRL expires in under 72 hours; push-crl.sh will refuse it, renew it now",
        ),
        "renew_soon" => ("warning", "the CRL expires in under 7 days"),
        _ => ("warning", "the CRL lifetime could not be read"),
    };
    json!([{
        "id": "crl",
        "state": state,
        "severity": severity,
        "seconds_remaining": remaining,
        "text": text,
        "next": "run baft-pki.sh renew-crl on the offline CA, check it with crl-status.sh, then distribute it with push-crl.sh",
    }])
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
    let units = ctx.units.clone();
    match tokio::task::spawn_blocking(move || units.state(&name)).await {
        Ok(st) => {
            let result = match st.active.as_str() {
                "active" => "active",
                "unknown" | "" => "unknown",
                _ => "inactive",
            };
            (
                json!({
                    "result": result,
                    "active_state": st.active,
                    "sub_state": st.sub,
                    "restarts": st.restarts,
                    "enabled_at_boot": st.enabled == "enabled",
                    "unit_file_state": if st.enabled.is_empty() { "unknown" } else { st.enabled.as_str() },
                }),
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

fn last_integrity_probe(ctx: &Ctx) -> Option<Value> {
    let dir = ctx.state_dir.as_ref()?;
    let mut latest: Option<Value> = None;
    for name in [AUDIT_ROTATED, AUDIT_FILE] {
        for line in audit_tail(&dir.join(name)).lines() {
            let Ok(event) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if event["action"] != "integrity_probe" {
                continue;
            }
            let timestamp = event["ts"].as_u64().unwrap_or(0);
            if latest
                .as_ref()
                .and_then(|value| value["timestamp"].as_u64())
                .is_none_or(|current| timestamp >= current)
            {
                let Some(result) = event["result"].as_str() else {
                    continue;
                };
                if !matches!(result, "pass" | "fail" | "unknown") {
                    continue;
                }
                latest = Some(json!({
                    "result": result,
                    "reason": event["detail"],
                    "timestamp": timestamp,
                    "source": "in_product_probe",
                }));
            }
        }
    }
    latest
}

const INTEGRITY_UNKNOWN: &str = "no recent in-product integrity probe; run Test tunnel explicitly (ready remains Unknown)";

fn integrity_tier(last_probe: Option<Value>, now: u64) -> Value {
    let timestamp = last_probe
        .as_ref()
        .and_then(|probe| probe["timestamp"].as_u64());
    let age = timestamp.map(|at| now.saturating_sub(at));
    let fresh = timestamp.is_some_and(|at| at <= now)
        && age.is_some_and(|seconds| seconds <= DATA_WINDOW_S);
    let result = if fresh {
        last_probe.as_ref().and_then(|probe| probe["result"].as_str()).unwrap_or("unknown")
    } else {
        "unknown"
    };
    let reason = if fresh {
        last_probe.as_ref().and_then(|probe| probe["reason"].as_str()).unwrap_or("probe_result_unavailable")
    } else if last_probe.is_some() {
        "stale_probe"
    } else {
        INTEGRITY_UNKNOWN
    };
    json!({
        "result": result,
        "reason": reason,
        "source": if fresh { Some("in_product_probe") } else { None },
        "timestamp": if fresh { timestamp } else { None },
        "age_s": age,
        "freshness_s": DATA_WINDOW_S,
        "last_probe": last_probe,
    })
}

/// End-to-end tunnel state as separate evidence tiers (ADR-0024). The summary `state` is the
/// highest tier actually observed; it never says "ready" and never implies integrity.
async fn tunnel_status(ctx: &Ctx, config: &RuntimeConfig, listening: bool) -> Value {
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
    let has_listener = !config.listen_addr().is_empty();
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
    let last_probe = last_integrity_probe(ctx);
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
            "integrity": integrity_tier(last_probe, now),
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

fn tier_with_params(
    id: &str,
    result: &str,
    source: &str,
    evidence: String,
    params: Value,
) -> Value {
    json!({"id": id, "result": result, "source": source, "evidence": evidence,
           "code": format!("diag.{id}.{result}"), "params": params})
}

fn tier(id: &str, result: &str, source: &str, evidence: String) -> Value {
    tier_with_params(id, result, source, evidence, json!({}))
}

const STAGES: [&str; 4] = ["tcp", "secure_channel", "identity", "authorization"];

/// One operator action for a tier that did not pass. `None` when the tier passed or there is
/// nothing to do on this node.
fn tier_next(id: &str, result: &str, dials_peer: bool) -> Option<&'static str> {
    if !matches!(result, "fail" | "unknown") || id == "integrity" {
        return None;
    }
    let fail = result == "fail";
    Some(match (id, fail) {
        ("tcp_reachability", true) => {
            "the peer's tunnel port does not answer: check that the peer is started, its address and port, and any firewall between the nodes"
        }
        ("tcp_reachability", false) if dials_peer => "start the peer and run diagnostics again",
        ("tcp_reachability", false) => return None,
        ("secure_channel", true) => {
            "the TLS/Noise handshake failed: check that both nodes use the same engine and that certificates, CA and CRL are valid (renew the CRL if it expired)"
        }
        ("identity", true) => {
            "the peer's identity did not match: compare the pinned fingerprint or Noise key with the peer's bundle checksum and re-run join if it changed"
        }
        ("authorization", true) => {
            "the peer was not authorized: check the peer id, tunnel id and that its certificate is not revoked"
        }
        ("session", true) => {
            "the unit runs but no peer is connected: start the peer node, then run diagnostics again"
        }
        ("data_path", _) => {
            "send traffic through the tunnel (open the service through it), then run diagnostics again"
        }
        (_, _) => return None,
    })
}

/// The single next step for the whole report: the unit first, then the first tier (in order)
/// that did not pass, which is what blocks every later tier.
fn report_next(unit: &Value, tiers: &[Value], verified: bool) -> Value {
    match unit["result"].as_str() {
        Some("inactive") => {
            return json!({"action": "start", "code": "diag.next.start", "params": {}, "text": "the tunnel unit is not running: press Start (it is then enabled at boot)"});
        }
        Some("unknown") | None => {
            return json!({"action": "check", "code": "diag.next.check", "params": {}, "text": "the unit state could not be read: check baft-tunnel@NAME with systemctl status on this host, then run diagnostics again"});
        }
        _ => {}
    }
    if verified {
        let integrity_result = tiers
            .iter()
            .find(|item| item["id"] == "integrity")
            .and_then(|item| item["result"].as_str())
            .unwrap_or("unknown");
        let text = match integrity_result {
            "pass" => "the in-product 64 KiB integrity probe passed for this peer, tunnel and time; Apply ready remains Unknown",
            "fail" => "the latest integrity probe failed; inspect the reported reason and test again after correcting the path",
            _ => "the data path was observed; integrity is Unknown or stale—use the explicit Test tunnel action",
        };
        let code = match integrity_result {
            "pass" => "diag.next.integrity_pass",
            "fail" => "diag.next.integrity_fail",
            _ => "diag.next.integrity_unknown",
        };
        return json!({"action": "none", "code": code, "params": {}, "text": text});
    }
    let blocker = tiers.iter().find(|t| {
        t["id"] != "integrity" && !matches!(t["result"].as_str(), Some("pass" | "not_applicable"))
    });
    match blocker {
        Some(t) if t.get("next").is_some() => {
            json!({"action": "fix", "tier": t["id"], "code": t["next_code"], "params": t["next_params"], "text": t["next"]})
        }
        Some(t) => {
            json!({"action": "rerun", "tier": t["id"], "code": "diag.next.rerun_tier", "params": {"tier": t["id"]}, "text": "no evidence for this tier yet: wait a minute (or start the peer), then run diagnostics again"})
        }
        None => {
            json!({"action": "rerun", "code": "diag.next.rerun", "params": {}, "text": "run diagnostics again in a minute"})
        }
    }
}

/// Peer-aware diagnostics (ADR-0024): TCP reachability, secure channel, identity, authorization,
/// session and data path are separate tiers, each with its own source. A tier is `pass` only on
/// direct evidence; otherwise it is `fail` (a recorded failure at that stage) or `unknown`.
async fn run_diagnostics(ctx: &Ctx, config: &RuntimeConfig) -> Value {
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
    let noise = config.engine_name() == "noise-tcp";
    let mut tiers = Vec::new();

    tiers.push(match (session, config.remote()) {
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
                tier_with_params(
                    "tcp_reachability",
                    result,
                    "active_tcp_probe",
                    format!("{} (who answers is not verified)", c["evidence"].as_str().unwrap_or("")),
                    json!({"port": addr.port()}),
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
            Some(i) if i >= 2 => tier_with_params(
                "tcp_reachability", "pass", "last_failure",
                format!("an enrolled client reached this node over TCP at {failed_at}; a later stage failed"),
                json!({"at_unix": failed_at}),
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
                Some(f) if f == i && f == 1 && config.remote().is_none() => tier(
                    id,
                    "unknown",
                    "last_failure",
                    format!(
                        "an inbound connection failed the handshake at {failed_at}; the sender is not identified"
                    ),
                ),
                Some(f) if f == i => tier_with_params(
                    id,
                    "fail",
                    "last_failure",
                    format!("the latest connection attempt failed at this stage at {failed_at}"),
                    json!({"at_unix": failed_at}),
                ),
                Some(f) if f > i => tier_with_params(
                    id,
                    "pass",
                    "last_failure",
                    format!("passed in the attempt at {failed_at}; a later stage failed"),
                    json!({"at_unix": failed_at}),
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
        Some(s) => tier_with_params(
            "session", "pass", "unit_session",
            format!("{} authorized session(s) with peer {} since {}", s["authorized_sessions"], s["peer_id"].as_str().unwrap_or("?"), since),
            json!({"sessions": s["authorized_sessions"], "peer_id": s["peer_id"], "since_unix": since}),
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
        (Some(_), Some(t)) if now.saturating_sub(t) <= DATA_WINDOW_S => tier_with_params("data_path", "pass", "unit_counters", format!("payload bytes were relayed in both directions at {t}"), json!({"at_unix": t})),
        (Some(_), _) => tier_with_params("data_path", "unknown", "unit_counters", format!("no payload in both directions within {DATA_WINDOW_S} s; send traffic through the tunnel and run again"), json!({"window_seconds": DATA_WINDOW_S})),
    });
    let integrity = integrity_tier(last_integrity_probe(ctx), now);
    tiers.push(tier_with_params(
        "integrity",
        integrity["result"].as_str().unwrap_or("unknown"),
        integrity["source"].as_str().unwrap_or("none"),
        integrity["reason"].as_str().unwrap_or(INTEGRITY_UNKNOWN).to_owned(),
        json!({
            "timestamp": integrity["timestamp"],
            "age_s": integrity["age_s"],
            "freshness_s": integrity["freshness_s"],
            "last_probe": integrity["last_probe"],
        }),
    ));

    let mut highest = Value::Null;
    for t in &tiers {
        match t["result"].as_str() {
            Some("pass") => highest = t["id"].clone(),
            Some("not_applicable") => {}
            _ => break,
        }
    }
    let verified = highest == "data_path" || highest == "integrity";
    let remote = config.remote().is_some();
    for t in &mut tiers {
        if let Some(n) = tier_next(
            t["id"].as_str().unwrap_or(""),
            t["result"].as_str().unwrap_or(""),
            remote,
        ) {
            t["next"] = json!(n);
            let id = t["id"].as_str().unwrap_or("");
            let result = t["result"].as_str().unwrap_or("");
            t["next_code"] = json!(format!("diag.next.{id}.{result}"));
            t["next_params"] = t["params"].clone();
        }
    }
    // Sending traffic only helps once a session exists; before that the session tier is the blocker.
    if session.is_none() {
        for t in &mut tiers {
            if t["id"] == "data_path" {
                if let Some(o) = t.as_object_mut() {
                    o.remove("next");
                }
            }
        }
    }
    let next_action = report_next(&unit, &tiers, verified);
    let previous = stored_diagnostics(ctx).and_then(|d| d["verified_unix"].as_u64());
    let report = json!({
        "diagnostics_version": 1,
        "at_unix": now,
        "engine": config.engine_name(),
        "unit": unit,
        "tiers": tiers,
        "highest_passed_tier": highest,
        "verified": verified,
        "verified_unix": if verified { Some(now) } else { previous },
        "next_action": next_action,
        "note": "each tier is separate evidence; TCP reachability alone never means a working tunnel; integrity is Pass only for a recent explicit authenticated probe, and ready remains unknown",
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
        421 => "Misdirected Request",
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
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:18080\r\nContent-Length: {}\r\n",
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

    async fn raw(req: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(serve(
            listener,
            Arc::new(Ctx::new(sample(), TOKEN.into(), None)),
        ));
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out).await;
        task.abort();
        out
    }

    #[test]
    fn only_loopback_host_names_are_accepted() {
        for ok in [
            "127.0.0.1",
            "127.0.0.1:18080",
            "localhost:9999",
            "LOCALHOST",
            "[::1]:18080",
            "[::1]",
        ] {
            assert!(
                allowed_host(&format!("GET / HTTP/1.1\r\nHost: {ok}\r\n\r\n")),
                "{ok}"
            );
        }
        for bad in [
            "evil.example",
            "evil.example:18080",
            "127.0.0.1.evil.example",
            "localhost.evil.example:18080",
            "[::2]:18080",
            "",
        ] {
            assert!(
                !allowed_host(&format!("GET / HTTP/1.1\r\nHost: {bad}\r\n\r\n")),
                "{bad}"
            );
        }
        assert!(allowed_host("GET / HTTP/1.0\r\n\r\n"));
    }

    #[tokio::test]
    async fn a_rebinding_host_is_refused_before_auth_and_assets() {
        for path in ["/", "/v1/health", "/v1/status"] {
            let out = raw(&format!(
                "GET {path} HTTP/1.1\r\nHost: rebind.example:18080\r\nAuthorization: Bearer {TOKEN}\r\n\r\n"
            ))
            .await;
            assert!(out.starts_with("HTTP/1.1 421 "), "{path}: {out}");
        }
        let out = raw(&format!(
            "GET /v1/status HTTP/1.1\r\nHost: localhost:18080\r\nAuthorization: Bearer {TOKEN}\r\n\r\n"
        ))
        .await;
        assert!(out.starts_with("HTTP/1.1 200 "), "{out}");
    }

    #[tokio::test]
    async fn a_trickling_client_is_dropped_within_one_io_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(serve(
            listener,
            Arc::new(Ctx::new(sample(), TOKEN.into(), None)),
        ));
        let mut s = TcpStream::connect(addr).await.unwrap();
        let started = std::time::Instant::now();
        let mut closed = false;
        // One byte every second never completes the head; the agent must close the connection.
        for b in b"GET /v1/health HTTP/1.1\r\nX-Slow: aaaaaaaaaaaaaaaaaaaa" {
            if s.write_all(&[*b]).await.is_err() {
                closed = true;
                break;
            }
            let mut probe = [0u8; 1];
            if let Ok(Ok(0)) = timeout(Duration::from_secs(1), s.read(&mut probe)).await {
                closed = true;
                break;
            }
        }
        task.abort();
        assert!(closed, "connection still open");
        assert!(started.elapsed() < IO_TIMEOUT + Duration::from_secs(3));
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
        let missing = std::env::temp_dir().join(format!("baft-pf-missing-{}", std::process::id()));
        let body = plan_body(&missing.to_string_lossy());
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
        for check in v["checks"].as_array().unwrap() {
            assert!(
                check["code"]
                    .as_str()
                    .is_some_and(|code| code.starts_with("pf.")),
                "{check}"
            );
            assert!(check["params"].is_object(), "{check}");
        }
        for id in ["peer_reachability", "peer_identity", "firewall"] {
            assert_eq!(result(id).as_deref(), Some("unknown"), "{id}");
        }
        assert_eq!(result("directory_writable").as_deref(), Some("pass"));
        assert!(!missing.exists());
    }

    #[test]
    fn crl_state_warns_a_week_ahead_and_never_invents_ok() {
        assert_eq!(crl_state(None), "unknown");
        assert_eq!(crl_state(Some(-1)), "expired");
        assert_eq!(crl_state(Some(0)), "expired");
        assert_eq!(crl_state(Some(71 * 3600)), "renew_now");
        assert_eq!(crl_state(Some(3 * 86400)), "renew_soon");
        assert_eq!(crl_state(Some(7 * 86400 - 1)), "renew_soon");
        assert_eq!(crl_state(Some(7 * 86400)), "ok");
        assert_eq!(crl_state(Some(30 * 86400)), "ok");
    }

    #[test]
    fn directory_writable_blocks_where_the_agent_cannot_create_files() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("baft-pf-w-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        assert_eq!(directory_writable_check(&base)["result"], "pass");
        // A missing directory is judged by its parent, which setup would create it in.
        assert_eq!(
            directory_writable_check(&base.join("new"))["result"],
            "pass"
        );
        assert_eq!(
            directory_writable_check(&base.join("a/b"))["result"],
            "unknown"
        );
        let ro = base.join("ro");
        std::fs::create_dir(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o550)).unwrap();
        // SAFETY: geteuid has no preconditions and cannot fail.
        if unsafe { libc::geteuid() } != 0 {
            let c = directory_writable_check(&ro);
            assert_eq!(c["result"], "blocked", "{c}");
            assert!(c["evidence"].as_str().unwrap().contains("as root"));
        }
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o750)).unwrap();
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[tokio::test]
    async fn preflight_warns_for_missing_pki_and_blocks_invalid_plans() {
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
        assert_ne!(v["overall"], "blocked", "{out}");
        let pki = v["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["id"] == "pki_files")
            .unwrap();
        assert_eq!(pki["result"], "warning", "{out}");
        assert!(
            pki["evidence"]
                .as_str()
                .unwrap()
                .contains("not installed yet")
        );
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
        assert_eq!(up["params"]["port"], open);
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
        assert!(
            !c["remediation"]
                .as_str()
                .unwrap_or_default()
                .contains("Reverse preview"),
            "blocked Reverse preflight must not advertise a preview: {c}"
        );
        assert!(
            c["remediation"]
                .as_str()
                .unwrap_or_default()
                .contains("Direct"),
            "blocked Reverse preflight should direct the operator to Direct: {c}"
        );
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
    async fn tcp_tls_preflight_allows_empty_or_csr_pending_credentials_for_panel_enrollment() {
        let empty = state_dir("pki-preflight-empty");
        let body = plan_body(&empty.to_string_lossy()).replace("noise-tcp", "tcp-tls");
        let (code, out) = call(None, "POST", "/v1/setup/preflight", Some(TOKEN), &body).await;
        assert_eq!(code, 200, "{out}");
        let value: Value = serde_json::from_str(&out[out.find('{').unwrap()..]).unwrap();
        let pki = value["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["id"] == "pki_files")
            .unwrap();
        assert_eq!(pki["result"], "warning", "{out}");

        // A generated CSR plus its metadata is a valid pending enrollment state after reload.
        for name in ["node.key", "node.csr", "node.csr.meta"] {
            std::fs::write(empty.join(name), b"test-placeholder").unwrap();
        }
        let (code, out) = call(None, "POST", "/v1/setup/preflight", Some(TOKEN), &body).await;
        assert_eq!(code, 200, "{out}");
        let value: Value = serde_json::from_str(&out[out.find('{').unwrap()..]).unwrap();
        let pki = value["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["id"] == "pki_files")
            .unwrap();
        assert_eq!(pki["result"], "warning", "{out}");
    }

    #[tokio::test]
    async fn tcp_tls_preflight_still_blocks_partial_or_invalid_credentials() {
        let dir = state_dir("pki-preflight-partial");
        std::fs::write(dir.join("node.key"), b"partial-key").unwrap();
        let body = plan_body(&dir.to_string_lossy()).replace("noise-tcp", "tcp-tls");
        let (code, out) = call(None, "POST", "/v1/setup/preflight", Some(TOKEN), &body).await;
        assert_eq!(code, 200, "{out}");
        let value: Value = serde_json::from_str(&out[out.find('{').unwrap()..]).unwrap();
        let pki = value["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["id"] == "pki_files")
            .unwrap();
        assert_eq!(pki["result"], "blocked", "{out}");
    }

    #[tokio::test]
    async fn preflight_blocks_tcp_tls_reverse_until_independent_acceptance() {
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
        assert_eq!(c["result"], "blocked", "{out}");
        assert!(
            !c["remediation"]
                .as_str()
                .unwrap_or_default()
                .contains("Reverse preview"),
            "tcp-tls Reverse preflight must not advertise a preview: {c}"
        );
        assert!(
            c["remediation"]
                .as_str()
                .unwrap_or_default()
                .contains("Direct"),
            "tcp-tls Reverse preflight should direct the operator to Direct: {c}"
        );
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
        assert_eq!(body["crl_state"], "unknown");
        let (_, st) = get_json(&ctx, "/v1/status").await;
        assert_eq!(st["warnings"][0]["id"], "crl", "{st}");
        assert_eq!(st["warnings"][0]["state"], "unknown");
        assert!(
            st["warnings"][0]["next"]
                .as_str()
                .unwrap()
                .contains("renew-crl")
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
        assert_eq!(body["engines"].as_array().unwrap().len(), 3);
        let bare = Ctx::new(sample(), TOKEN.into(), None);
        assert_eq!(get_json(&bare, "/v1/audit").await.0, 404);
        let unauth = route("GET /v1/history HTTP/1.1\r\n\r\n", b"", &bare).await;
        assert_eq!(unauth.0, 401);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn wizard_strings_for_baft_and_reverse_exist_in_both_languages_and_never_claim_ready() {
        let i18n = include_str!("../panel/i18n.js");
        let (fa, en) = i18n.split_once("    en: {").expect("two language blocks");
        for key in [
            "role.rev.serve",
            "role.rev.forward",
            "srv.rev.serve",
            "srv.rev.serve.d",
            "srv.rev.forward",
            "srv.rev.forward.d",
            "f.port.ingress",
            "pv.ingress",
            "meth.baft",
            "meth.baft.limits",
            "meth.rev.note",
        ] {
            let needle = format!("'{key}':");
            assert!(fa.contains(&needle), "fa missing {key}");
            assert!(en.contains(&needle), "en missing {key}");
        }
        let pages = include_str!("../panel/pages.js");
        assert!(!pages.contains("meth.ready"));
        assert!(en.contains(
            "'meth.rev.note': 'Reverse is unavailable pending independent implementation and acceptance.'"
        ));
    }

    #[test]
    fn lifecycle_and_next_action_strings_exist_in_both_languages() {
        let i18n = include_str!("../panel/i18n.js");
        let (fa, en) = i18n.split_once("    en: {").expect("two language blocks");
        for key in [
            "life.state",
            "life.boot",
            "life.boot.on",
            "life.boot.off",
            "life.act.start",
            "life.act.stop",
            "life.act.restart",
            "life.confirm.start",
            "life.confirm.stop",
            "life.confirm.restart",
            "life.rolled",
            "life.noinstance",
            "diag.col.next",
            "diag.next",
            "diag.next.start",
            "verify.title",
            "verify.run",
            "verify.pass",
            "verify.pass.prefix",
            "verify.pass.bytes",
            "verify.pass.duration",
            "verify.reason.unsupported_protocol",
            "diag.next.integrity_pass",
            "diag.next.integrity_fail",
            "diag.next.integrity_unknown",
            "verify.reason.control_unavailable",
            "verify.fail",
            "verify.last",
            "verify.result.pass",
            "verify.result.fail",
            "verify.result.unknown",
        ] {
            let needle = format!("'{key}':");
            assert!(fa.contains(&needle), "fa missing {key}");
            assert!(en.contains(&needle), "en missing {key}");
        }
        let pages = include_str!("../panel/pages.js");
        assert!(pages.contains("'/v1/verify'"));
        assert!(pages.contains("verify.title"));
        assert!(pages.contains("'/v1/tunnel/lifecycle'"));
        assert!(pages.contains("window.confirm(t('life.confirm.' + action))"));
    }

    #[test]
    fn integrity_tier_passes_only_for_a_recent_probe_and_preserves_history_when_stale() {
        let recent = json!({
            "result": "pass",
            "reason": "sha256_verified",
            "timestamp": 950,
            "source": "in_product_probe"
        });
        let fresh = integrity_tier(Some(recent.clone()), 1_000);
        assert_eq!(fresh["result"], "pass");
        assert_eq!(fresh["source"], "in_product_probe");
        assert_eq!(fresh["age_s"], 50);
        assert_eq!(fresh["freshness_s"], DATA_WINDOW_S);
        assert_eq!(fresh["last_probe"]["result"], "pass");

        let stale = integrity_tier(Some(recent), 1_011);
        assert_eq!(stale["result"], "unknown");
        assert_eq!(stale["reason"], "stale_probe");
        assert_eq!(stale["source"], Value::Null);
        assert_eq!(stale["last_probe"]["result"], "pass");

        let failed = integrity_tier(
            Some(json!({"result":"fail","reason":"integrity_mismatch","timestamp":999})),
            1_000,
        );
        assert_eq!(failed["result"], "fail");
        let missing = integrity_tier(None, 1_000);
        assert_eq!(missing["result"], "unknown");
        assert_eq!(missing["reason"], INTEGRITY_UNKNOWN);
    }

    #[tokio::test]
    async fn integrity_verify_route_is_authenticated_idempotent_and_audits_unknown_without_a_control_socket() {
        let dir = state_dir("verify");
        let runtime = dir.join("runtime");
        std::fs::create_dir(&runtime).unwrap();
        let ctx = Ctx::new(sample(), TOKEN.into(), Some(dir.clone()))
            .with_unit(Some("verify-node".into()), Some(runtime));
        let request_body = json!({"idempotency_key":"verify-test-0001"}).to_string();
        let (status, response) = route(&post("/v1/verify"), request_body.as_bytes(), &ctx).await;
        assert_eq!(status, 200);
        assert_eq!(response["result"], "unknown");
        assert_eq!(response["reason"], "control_unavailable");
        assert_eq!(response["bytes"], 0);
        assert_eq!(response["sha256_match"], false);
        assert_eq!(response["peer_id"], sample().ids().1);
        assert_eq!(response["tunnel_id"], sample().ids().2);
        assert_eq!(response["direction"], sample().direction_name());

        let audit_path = dir.join(AUDIT_FILE);
        let lines = std::fs::read_to_string(&audit_path).unwrap();
        let event: Value = serde_json::from_str(lines.lines().next().unwrap()).unwrap();
        assert_eq!(event["action"], "integrity_probe");
        assert_eq!(event["result"], "unknown");
        assert_eq!(event["detail"], "control_unavailable");
        assert_eq!(event["resource"], "integrity_probe/tunnel/demo/peer/iran/direction/direct");
        assert!(event["sha256"].as_str().is_some_and(|hash| hash.len() == 64));
        assert!(event["receipt_id"].as_str().is_some());
        assert!(response.get("sha256").is_none());

        let (replay_status, replay) = route(&post("/v1/verify"), request_body.as_bytes(), &ctx).await;
        assert_eq!(replay_status, 200);
        assert_eq!(replay["result"], "unknown");
        assert_eq!(replay["replayed"], true);
        let lines_after_replay = std::fs::read_to_string(&audit_path).unwrap();
        assert_eq!(lines_after_replay.lines().count(), 1);
        assert!(!lines_after_replay.contains(TOKEN));

        let no_runtime_dir = state_dir("verify-no-runtime");
        let no_runtime = Ctx::new(sample(), TOKEN.into(), Some(no_runtime_dir.clone()))
            .with_unit(Some("verify-node".into()), None);
        let no_runtime_body = json!({"idempotency_key":"verify-no-run-0001"}).to_string();
        let (status, response) = route(&post("/v1/verify"), no_runtime_body.as_bytes(), &no_runtime).await;
        assert_eq!(status, 200);
        assert_eq!(response["result"], "unknown");
        assert_eq!(response["reason"], "control_unavailable");
        let event: Value = serde_json::from_str(
            std::fs::read_to_string(no_runtime_dir.join(AUDIT_FILE))
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(event["action"], "integrity_probe");
        assert_eq!(event["result"], "unknown");
        let _ = std::fs::remove_dir_all(&no_runtime_dir);

        let bare = Ctx::new(sample(), TOKEN.into(), None);
        assert_eq!(
            route("POST /v1/verify HTTP/1.1\r\n\r\n", b"", &bare)
                .await
                .0,
            401
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn latest_probe_audit_is_exposed_as_historical_evidence() {
        let dir = state_dir("last-probe");
        let ctx = Ctx::new(sample(), TOKEN.into(), Some(dir.clone()));
        audit(
            &dir,
            &ctx,
            AuditEvent {
                action: "integrity_probe",
                result: "pass",
                resource: "tunnel/demo".into(),
                node: None,
                instance: None,
                sha256: "test-digest",
                detail: "sha256_verified",
                receipt_id: None,
            },
        )
        .unwrap();
        let last = last_integrity_probe(&ctx).unwrap();
        assert_eq!(last["result"], "pass");
        assert_eq!(last["source"], "in_product_probe");
        assert!(last["timestamp"].as_u64().is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn audit_log_rotates_and_the_endpoint_reads_only_a_bounded_tail() {
        let dir = state_dir("auditrot");
        let ctx = Ctx::new(sample(), TOKEN.into(), Some(dir.clone()));
        let ev = |detail: &'static str| AuditEvent {
            action: "put_desired",
            result: "accepted",
            resource: "desired_state".into(),
            node: None,
            instance: None,
            sha256: "abc",
            detail,
            receipt_id: None,
        };
        // Pre-fill past the cap with valid lines, then one more append must rotate.
        let big = "{\"audit_version\":2,\"detail\":\"old\"}\n";
        let n = (AUDIT_MAX_BYTES as usize / big.len()) + 1;
        std::fs::write(dir.join(AUDIT_FILE), big.repeat(n)).unwrap();
        audit(&dir, &ctx, ev("new")).unwrap();
        assert!(dir.join(AUDIT_ROTATED).exists(), "rotated generation kept");
        let live = std::fs::metadata(dir.join(AUDIT_FILE)).unwrap();
        assert!(live.len() < 4096, "live file restarted small");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(live.permissions().mode() & 0o777, 0o600);
        // The endpoint returns the newest 100 (old tail + the new line), never the whole file.
        let (code, body) = get_json(&ctx, "/v1/audit").await;
        assert_eq!(code, 200);
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 100);
        assert_eq!(entries.last().unwrap()["detail"], "new");
        // A cut first line in the tail is dropped, not misparsed.
        let tail = audit_tail(&dir.join(AUDIT_ROTATED));
        assert!(tail.len() as u64 <= AUDIT_TAIL_BYTES);
        assert!(
            tail.lines()
                .all(|l| serde_json::from_str::<Value>(l).is_ok())
        );
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
        for tier in d["tiers"].as_array().unwrap() {
            assert!(
                tier["code"]
                    .as_str()
                    .is_some_and(|code| code.starts_with("diag.")),
                "{tier}"
            );
            assert!(tier["params"].is_object(), "{tier}");
        }
        assert_eq!(d["tiers"][4]["code"], "diag.session.pass");
        assert_eq!(d["tiers"][4]["params"]["peer_id"], "iran");
        assert_eq!(d["next_action"]["code"], "diag.next.check"); // The test unit has no readable systemd state; never hide that Unknown.
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
        assert_eq!(d["tiers"][2]["code"], "diag.identity.fail");
        assert_eq!(d["tiers"][2]["next_code"], "diag.next.identity.fail");
        assert_eq!(d["next_action"]["code"], "diag.next.check"); // Unit state is Unknown, so the unit-level action takes precedence.
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

    struct AgentFake {
        st: std::sync::Mutex<crate::apply::UnitState>,
        log: std::sync::Mutex<Vec<String>>,
    }

    impl crate::apply::Units for AgentFake {
        fn request(&self, name: &str, verb: &str) -> Result<(), String> {
            self.log.lock().unwrap().push(format!("{verb} {name}"));
            let mut st = self.st.lock().unwrap();
            if verb == "disable" {
                st.active = "inactive".into();
                st.enabled = "disabled".into();
            }
            Ok(())
        }
        fn state(&self, _: &str) -> crate::apply::UnitState {
            self.st.lock().unwrap().clone()
        }
        fn settle(&self, _: Duration) {}
    }

    fn lifecycle_ctx(dir: &Path, instance: Option<&str>, active: &str) -> (Ctx, Arc<AgentFake>) {
        let fake = Arc::new(AgentFake {
            st: std::sync::Mutex::new(crate::apply::UnitState {
                active: active.into(),
                sub: "running".into(),
                enabled: "enabled".into(),
                ..Default::default()
            }),
            log: std::sync::Mutex::new(vec![]),
        });
        let ctx = Ctx::new(sample(), TOKEN.into(), Some(dir.to_path_buf()))
            .with_unit(instance.map(str::to_string), None)
            .with_units(fake.clone(), dir.to_path_buf());
        (ctx, fake)
    }

    fn post(path: &str) -> String {
        format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {TOKEN}")
    }

    #[tokio::test]
    async fn lifecycle_needs_token_instance_confirm_and_a_lifecycle_action() {
        let dir = state_dir("life");
        let (ctx, fake) = lifecycle_ctx(&dir, None, "active");
        let body = br#"{"action":"stop","confirm":true,"idempotency_key":"life-0001"}"#;
        let head = "POST /v1/tunnel/lifecycle HTTP/1.1\r\nHost: 127.0.0.1";
        assert_eq!(route(head, body, &ctx).await.0, 401);
        assert_eq!(
            route(&post("/v1/tunnel/lifecycle"), body, &ctx).await.0,
            409
        );
        let (ctx, _) = lifecycle_ctx(&dir, Some("edge"), "active");
        for bad in [
            r#"{"action":"stop","confirm":false,"idempotency_key":"life-0001"}"#,
            r#"{"action":"init","confirm":true,"idempotency_key":"life-0001"}"#,
            r#"{"action":"reboot","confirm":true,"idempotency_key":"life-0001"}"#,
            r#"{"action":"stop","confirm":true,"idempotency_key":"x"}"#,
        ] {
            assert_eq!(
                route(&post("/v1/tunnel/lifecycle"), bad.as_bytes(), &ctx)
                    .await
                    .0,
                422,
                "{bad}"
            );
        }
        assert!(fake.log.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn lifecycle_stop_writes_a_receipt_and_an_audit_line() {
        let dir = state_dir("life-stop");
        let (ctx, fake) = lifecycle_ctx(&dir, Some("edge"), "active");
        let body = br#"{"action":"stop","confirm":true,"idempotency_key":"life-stop-1"}"#;
        let (code, out) = route(&post("/v1/tunnel/lifecycle"), body, &ctx).await;
        assert_eq!(code, 200, "{out}");
        assert_eq!(out["result"], "applied");
        assert_eq!(out["boot"]["enabled_at_boot"], false);
        assert_eq!(*fake.log.lock().unwrap(), vec!["disable edge"]);
        assert!(dir.join("receipts/life-stop-1.json").exists());
        let audit = std::fs::read_to_string(dir.join(AUDIT_FILE)).unwrap();
        assert!(audit.contains("\"resource\":\"tunnel/stop\""), "{audit}");
        assert!(!audit.contains(TOKEN));
    }

    #[tokio::test]
    async fn diagnostics_name_one_next_action() {
        let dir = state_dir("diag-next");
        let (ctx, _) = lifecycle_ctx(&dir, Some("edge"), "inactive");
        let (code, out) = route(&post("/v1/diagnostics"), b"", &ctx).await;
        assert_eq!(code, 200);
        assert_eq!(out["next_action"]["action"], "start", "{out}");
        assert_eq!(out["unit"]["enabled_at_boot"], true);
        assert!(
            out["tiers"]
                .as_array()
                .unwrap()
                .iter()
                .all(|t| t["id"] != "integrity" || t.get("next").is_none())
        );
        assert_eq!(
            tier_next("session", "fail", true),
            Some(
                "the unit runs but no peer is connected: start the peer node, then run diagnostics again"
            )
        );
        assert_eq!(tier_next("session", "pass", true), None);
        assert_eq!(tier_next("tcp_reachability", "unknown", false), None);
        let tiers = vec![
            json!({"id": "tcp_reachability", "result": "pass"}),
            json!({"id": "session", "result": "fail", "next": "x"}),
            json!({"id": "data_path", "result": "unknown", "next": "y"}),
        ];
        assert_eq!(
            report_next(&json!({"result": "active"}), &tiers, false)["tier"],
            "session"
        );
        assert_eq!(
            report_next(&json!({"result": "active"}), &[], true)["action"],
            "none"
        );
    }

    #[derive(Default)]
    struct PkiFake {
        requests: std::sync::Mutex<Vec<Value>>,
        unavailable: bool,
        refused: bool,
        replayed: bool,
        wrong_request_id: bool,
        export_private: bool,
    }

    impl crate::pki_helper::PkiHelperClient for PkiFake {
        fn available(&self, _: &str) -> std::result::Result<(), String> {
            if self.unavailable {
                Err("PKI helper unavailable in test".into())
            } else {
                Ok(())
            }
        }

        fn submit(&self, _: &str, request: &Value) -> std::result::Result<Value, String> {
            self.requests.lock().unwrap().push(request.clone());
            let operation = request["operation"].as_str().unwrap();
            let node_id = request["node_id"].as_str().unwrap();
            let server_name = request["server_name"].as_str().unwrap();
            let expected_san = crate::pki_helper::expected_server_san(server_name)
                .map_err(|error| format!("{error:#}"))?;
            if self.refused {
                return Ok(json!({
                    "receipt_version": 1,
                    "request_id": if self.wrong_request_id { json!("wrong-request") } else { request["request_id"].clone() },
                    "operation": operation,
                    "result": "refused",
                    "reason": "test refusal",
                    "private_key_exported": false
                }));
            }
            if operation == "install_certificate" {
                return Ok(json!({
                    "receipt_version": 1,
                    "request_id": if self.wrong_request_id { json!("wrong-request") } else { request["request_id"].clone() },
                    "operation": operation,
                    "result": "installed",
                    "node_id": node_id,
                    "certificate_sha256": "b".repeat(64),
                    "certificate_not_after": 1_800_000_000i64,
                    "crl_seconds_remaining": 7 * 24 * 3600,
                    "active_set": "c".repeat(64),
                    "private_key_exported": self.export_private
                }));
            }
            Ok(json!({
                "receipt_version": 1,
                "request_id": if self.wrong_request_id { json!("wrong-request") } else { request["request_id"].clone() },
                "operation": operation,
                "result": if operation == "create_csr" { "created" } else { "retrieved" },
                "node_id": node_id,
                "expected_server_san": expected_san,
                "public_key_sha256": "a".repeat(64),
                "csr_pem": "-----BEGIN CERTIFICATE REQUEST-----\\nZmFrZQ==\\n-----END CERTIFICATE REQUEST-----",
                "private_key_exported": self.export_private,
                "replayed": self.replayed
            }))
        }
    }

    /// Runs the real root helper code in-process against temporary directories.
    struct InProcessHelper {
        control: PathBuf,
        conf: PathBuf,
        receipts: PathBuf,
    }

    impl crate::setup_helper::SetupHelper for InProcessHelper {
        fn available(&self, _: &str) -> std::result::Result<(), String> {
            Ok(())
        }
        fn submit(&self, name: &str, request: &Value) -> std::result::Result<Value, String> {
            std::fs::write(
                self.control.join(format!("{name}.setup")),
                request.to_string(),
            )
            .map_err(|e| e.to_string())?;
            crate::setup_helper::run(name, &self.control, &self.conf, &self.receipts)
                .map_err(|e| format!("{e:#}"))
        }
    }

    fn setup_mode_ctx(base: &Path) -> (Ctx, PathBuf) {
        let conf = base.join("etc");
        let control = base.join("control");
        for d in [&conf, &control, &base.join("state")] {
            std::fs::create_dir_all(d).unwrap();
        }
        let helper = Arc::new(InProcessHelper {
            control,
            conf: conf.clone(),
            receipts: base.join("helper-receipts"),
        });
        let fake = Arc::new(AgentFake {
            st: std::sync::Mutex::new(crate::apply::UnitState::default()),
            log: std::sync::Mutex::new(vec![]),
        });
        let ctx = Ctx::with_config(None, TOKEN.into(), Some(base.join("state")))
            .with_unit(Some("pair1".into()), None)
            .with_units(fake, conf.clone())
            .with_setup_helper(helper);
        (ctx, conf)
    }

    #[tokio::test]
    async fn pki_certificate_upload_is_confirmed_bounded_audited_and_never_restarts_service() {
        let base = state_dir("pki-certificate-api");
        let (ctx, _conf) = setup_mode_ctx(&base);
        let fake = Arc::new(PkiFake::default());
        let ctx = ctx.with_pki_helper(fake.clone());
        let request = json!({
            "request_id": "cert-upload-01",
            "node_id": "abroad",
            "server_name": "abroad.lab",
            "confirm": true,
            "certificate_pem": "-----BEGIN CERTIFICATE-----\\nZmFrZQ==\\n-----END CERTIFICATE-----",
            "ca_pem": "-----BEGIN CERTIFICATE-----\\nQ0E=\\n-----END CERTIFICATE-----",
            "crl_pem": "-----BEGIN X509 CRL-----\\nQ1JM\\n-----END X509 CRL-----"
        });
        let body = request.to_string();
        let (status, installed) =
            route(&post("/v1/pki/certificate/install"), body.as_bytes(), &ctx).await;
        assert_eq!(status, 200, "{installed}");
        assert_eq!(installed["result"], "installed");
        assert_eq!(installed["service_restarted"], false);
        assert_eq!(installed["private_key_exported"], false);
        assert_eq!(installed["certificate_sha256"], "b".repeat(64));
        {
            let requests = fake.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0]["operation"], "install_certificate");
            assert!(
                requests[0]["certificate_pem"]
                    .as_str()
                    .unwrap()
                    .contains("BEGIN CERTIFICATE")
            );
            assert!(!requests[0].to_string().contains("PRIVATE KEY"));
        }

        let no_confirm = json!({
            "request_id": "cert-upload-02",
            "node_id": "abroad",
            "server_name": "abroad.lab",
            "confirm": false,
            "certificate_pem": "x",
            "ca_pem": "y",
            "crl_pem": "z"
        });
        assert_eq!(
            route(
                &post("/v1/pki/certificate/install"),
                no_confirm.to_string().as_bytes(),
                &ctx
            )
            .await
            .0,
            409
        );
        assert_eq!(
            route(
                &post("/v1/pki/certificate/install"),
                &vec![b' '; crate::pki_helper::MAX_REQUEST_BYTES as usize + 1],
                &ctx
            )
            .await
            .0,
            413
        );
        let audit_text = std::fs::read_to_string(base.join("state").join(AUDIT_FILE)).unwrap();
        assert!(audit_text.contains("install_certificate"));
        assert!(!audit_text.contains("BEGIN CERTIFICATE"));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn pki_csr_routes_are_authenticated_bounded_audited_and_keep_keys_local() {
        let base = state_dir("pki-api");
        let (ctx, _conf) = setup_mode_ctx(&base);
        let fake = Arc::new(PkiFake::default());
        let ctx = ctx.with_pki_helper(fake.clone());
        let create = json!({
            "request_id": "csr-create-01",
            "node_id": "abroad",
            "server_name": "abroad.lab",
            "confirm": true
        });
        let body = create.to_string();

        let no_auth = "POST /v1/pki/csr/create HTTP/1.1\r\nHost: 127.0.0.1";
        assert_eq!(route(no_auth, body.as_bytes(), &ctx).await.0, 401);
        assert_eq!(
            route(&get("/v1/pki/csr/create"), body.as_bytes(), &ctx)
                .await
                .0,
            405
        );
        assert_eq!(route(&get("/v1/pki/csr/unknown"), b"", &ctx).await.0, 404);
        assert_eq!(
            route(&post("/v1/pki/csr/create"), &vec![b' '; 4097], &ctx)
                .await
                .0,
            413
        );
        let no_confirm =
            json!({"request_id":"csr-create-02","node_id":"abroad","server_name":"abroad.lab"});
        assert_eq!(
            route(
                &post("/v1/pki/csr/create"),
                no_confirm.to_string().as_bytes(),
                &ctx
            )
            .await
            .0,
            422
        );
        let bad_san = json!({"request_id":"csr-create-03","node_id":"abroad","server_name":"*.lab","confirm":true});
        assert_eq!(
            route(
                &post("/v1/pki/csr/create"),
                bad_san.to_string().as_bytes(),
                &ctx
            )
            .await
            .0,
            422
        );

        let (code, created) = route(&post("/v1/pki/csr/create"), body.as_bytes(), &ctx).await;
        assert_eq!(code, 201, "{created}");
        assert_eq!(created["result"], "created");
        assert_eq!(created["private_key_exported"], false);
        assert_eq!(created["expected_server_san"], "DNS:abroad.lab");
        assert!(
            created["csr_pem"]
                .as_str()
                .unwrap()
                .contains("BEGIN CERTIFICATE REQUEST")
        );

        let read =
            json!({"request_id":"csr-read-01","node_id":"abroad","server_name":"abroad.lab"});
        let (code, retrieved) =
            route(&post("/v1/pki/csr/read"), read.to_string().as_bytes(), &ctx).await;
        assert_eq!(code, 200, "{retrieved}");
        assert_eq!(retrieved["result"], "retrieved");
        let requests = fake.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["operation"], "create_csr");
        assert_eq!(requests[1]["operation"], "read_csr");
        drop(requests);
        let audit_text = std::fs::read_to_string(base.join("state").join(AUDIT_FILE)).unwrap();
        assert!(audit_text.contains("pki_csr"));
        assert!(!audit_text.contains("BEGIN CERTIFICATE REQUEST"));
        assert!(!audit_text.contains("PRIVATE KEY"));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn pki_csr_helper_failures_and_untrusted_receipts_are_audited() {
        let create = json!({"request_id":"csr-create-05","node_id":"abroad","server_name":"abroad.lab","confirm":true});
        let body = create.to_string();

        let unavailable_dir = state_dir("pki-unavailable");
        let (ctx, _) = setup_mode_ctx(&unavailable_dir);
        let unavailable = Arc::new(PkiFake {
            unavailable: true,
            ..PkiFake::default()
        });
        let ctx = ctx.with_pki_helper(unavailable);
        assert_eq!(
            route(&post("/v1/pki/csr/create"), body.as_bytes(), &ctx)
                .await
                .0,
            503
        );
        let audit =
            std::fs::read_to_string(unavailable_dir.join("state").join(AUDIT_FILE)).unwrap();
        assert!(audit.contains("helper_unavailable"));

        let refused_dir = state_dir("pki-refused");
        let (ctx, _) = setup_mode_ctx(&refused_dir);
        let refused = Arc::new(PkiFake {
            refused: true,
            ..PkiFake::default()
        });
        let ctx = ctx.with_pki_helper(refused);
        assert_eq!(
            route(&post("/v1/pki/csr/create"), body.as_bytes(), &ctx)
                .await
                .0,
            409
        );
        let audit = std::fs::read_to_string(refused_dir.join("state").join(AUDIT_FILE)).unwrap();
        assert!(audit.contains(r#""result":"refused""#));

        let invalid_dir = state_dir("pki-invalid-receipt");
        let (ctx, _) = setup_mode_ctx(&invalid_dir);
        let invalid = Arc::new(PkiFake {
            wrong_request_id: true,
            ..PkiFake::default()
        });
        let ctx = ctx.with_pki_helper(invalid);
        assert_eq!(
            route(&post("/v1/pki/csr/create"), body.as_bytes(), &ctx)
                .await
                .0,
            502
        );
        let audit = std::fs::read_to_string(invalid_dir.join("state").join(AUDIT_FILE)).unwrap();
        assert!(audit.contains("invalid_receipt"));

        let export_dir = state_dir("pki-export-attempt");
        let (ctx, _) = setup_mode_ctx(&export_dir);
        let invalid = Arc::new(PkiFake {
            export_private: true,
            ..PkiFake::default()
        });
        let ctx = ctx.with_pki_helper(invalid);
        assert_eq!(
            route(&post("/v1/pki/csr/create"), body.as_bytes(), &ctx)
                .await
                .0,
            502
        );
        let audit = std::fs::read_to_string(export_dir.join("state").join(AUDIT_FILE)).unwrap();
        assert!(audit.contains("invalid_receipt"));

        let replay_dir = state_dir("pki-replayed");
        let (ctx, _) = setup_mode_ctx(&replay_dir);
        let replay = Arc::new(PkiFake {
            replayed: true,
            ..PkiFake::default()
        });
        let ctx = ctx.with_pki_helper(replay);
        assert_eq!(
            route(&post("/v1/pki/csr/create"), body.as_bytes(), &ctx)
                .await
                .0,
            200
        );

        for dir in [
            unavailable_dir,
            refused_dir,
            invalid_dir,
            export_dir,
            replay_dir,
        ] {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn pki_csr_setup_and_configured_modes_fail_closed_on_wrong_state() {
        let base = state_dir("pki-api-modes");
        let (ctx, conf) = setup_mode_ctx(&base);
        let fake = Arc::new(PkiFake::default());
        let ctx = ctx.with_pki_helper(fake.clone());
        let create = json!({"request_id":"csr-create-04","node_id":"abroad","server_name":"abroad.lab","confirm":true});
        std::fs::write(
            conf.join(crate::setup::STATE_FILE),
            json!({"engine":"noise-tcp","node_id":"abroad"}).to_string(),
        )
        .unwrap();
        assert_eq!(
            route(
                &post("/v1/pki/csr/create"),
                create.to_string().as_bytes(),
                &ctx
            )
            .await
            .0,
            409
        );
        assert!(fake.requests.lock().unwrap().is_empty());
        let state_path = conf.join(crate::setup::STATE_FILE);
        std::fs::remove_file(&state_path).unwrap();
        let victim = base.join("state-victim");
        std::fs::write(&victim, br#"{"engine":"tcp-tls","node_id":"abroad"}"#).unwrap();
        std::os::unix::fs::symlink(&victim, &state_path).unwrap();
        assert_eq!(
            route(
                &post("/v1/pki/csr/create"),
                create.to_string().as_bytes(),
                &ctx
            )
            .await
            .0,
            409
        );
        std::fs::remove_file(&state_path).unwrap();
        std::fs::write(&state_path, b"not-json").unwrap();
        assert_eq!(
            route(
                &post("/v1/pki/csr/create"),
                create.to_string().as_bytes(),
                &ctx
            )
            .await
            .0,
            409
        );
        assert!(fake.requests.lock().unwrap().is_empty());

        let missing_instance = Ctx::with_config(None, TOKEN.into(), Some(base.join("state")))
            .with_pki_helper(fake.clone());
        assert_eq!(
            route(
                &post("/v1/pki/csr/create"),
                create.to_string().as_bytes(),
                &missing_instance
            )
            .await
            .0,
            409
        );

        let state = state_dir("pki-configured");
        let configured = Ctx::new(serve_config(), TOKEN.into(), Some(state.clone()))
            .with_unit(Some("pair1".into()), None)
            .with_pki_helper(fake.clone());
        assert_eq!(
            route(
                &post("/v1/pki/csr/create"),
                create.to_string().as_bytes(),
                &configured
            )
            .await
            .0,
            409
        );
        let read =
            json!({"request_id":"csr-read-02","node_id":"abroad","server_name":"abroad.lab"});
        let (code, receipt) = route(
            &post("/v1/pki/csr/read"),
            read.to_string().as_bytes(),
            &configured,
        )
        .await;
        assert_eq!(code, 200, "{receipt}");
        std::fs::remove_dir_all(base).unwrap();
        std::fs::remove_dir_all(state).unwrap();
    }

    fn get(path: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {TOKEN}")
    }

    #[tokio::test]
    async fn panel_pairing_runs_init_and_join_through_the_root_helper_in_setup_mode() {
        let base = state_dir("pairing");
        let (ctx, conf) = setup_mode_ctx(&base);
        // Setup mode: reduced status, unrelated routes say so.
        let (code, st) = route(&get("/v1/status"), b"", &ctx).await;
        assert_eq!(code, 200);
        assert_eq!(st["setup_pending"], true);
        assert_eq!(st["instance"], "pair1");
        assert_eq!(st["pairing"]["initialized"], false);
        let (code, out) = route(&get("/v1/security"), b"", &ctx).await;
        assert_eq!((code, out["setup_pending"].clone()), (409, json!(true)));
        assert_eq!(route(&get("/v1/setup/bundle"), b"", &ctx).await.0, 404);

        let plan = json!({"engine": "noise-tcp", "role": "serve", "direction": "direct",
            "node_id": "abroad", "peer_id": "iran", "tunnel_id": "pairdemo", "service_port": 18180,
            "tunnel_port": 18181, "dir": conf, "instance": "pair1"});
        // Preflight checks the helper instead of the agent's own write access.
        let (_, pf) = route(
            &post("/v1/setup/preflight"),
            plan.to_string().as_bytes(),
            &ctx,
        )
        .await;
        let ids: Vec<&str> = pf["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        assert!(
            ids.contains(&"setup_helper") && !ids.contains(&"directory_writable"),
            "{ids:?}"
        );
        // Another directory or instance is refused before anything runs.
        let mut other = plan.clone();
        other["dir"] = json!("/tmp");
        let bad = json!({"stage": "init", "confirm": true, "idempotency_key": "pair-init-x", "plan": other});
        assert_eq!(
            route(&post("/v1/setup/apply"), bad.to_string().as_bytes(), &ctx)
                .await
                .0,
            422
        );

        let init = json!({"stage": "init", "confirm": true, "idempotency_key": "pair-init-1", "plan": plan});
        let (code, rc) = route(&post("/v1/setup/apply"), init.to_string().as_bytes(), &ctx).await;
        assert_eq!(code, 200, "{rc}");
        assert_eq!(rc["result"], "applied");
        assert_eq!(rc["source"], "root setup helper");
        assert_eq!(rc["receipt_stored"], true);
        let (_, again) = route(&post("/v1/setup/apply"), init.to_string().as_bytes(), &ctx).await;
        assert_eq!(again["replayed"], true, "same key is replayed by the agent");

        // Export.
        let (code, own) = route(&get("/v1/setup/bundle"), b"", &ctx).await;
        assert_eq!(code, 200);
        assert_eq!(own["checksum"], rc["pairing"]["checksum"]);
        assert!(!own["bundle"].as_str().unwrap().contains("private"));
        // Our own bundle is not a valid peer.
        let ins = json!({"bundle": own["bundle"], "checksum": own["checksum"]});
        let (_, v) = route(
            &post("/v1/setup/bundle/inspect"),
            ins.to_string().as_bytes(),
            &ctx,
        )
        .await;
        assert_eq!(v["checksum_match"], true);
        assert_eq!(v["matches_this_node"], false);

        // The peer prepares itself (CLI on the other host).
        let peer = base.join("peer");
        let peer_init = crate::setup::init(
            &crate::setup::PlanRequest {
                engine: crate::config::EngineKind::NoiseTcp,
                role: crate::setup::Role::Forward,
                direction: crate::authorization::ConnectionDirection::Direct,
                node_id: "iran".into(),
                peer_id: "abroad".into(),
                tunnel_id: "pairdemo".into(),
                service_port: 18180,
                tunnel_port: 18181,
                peer_host: Some("127.0.0.1".into()),
                server_name: None,
                dir: peer.clone(),
                instance: None,
            },
            None,
        )
        .unwrap();
        let peer_bundle = std::fs::read_to_string(peer.join("bundle.json")).unwrap();
        // Import preview: wrong checksum is flagged, right one matches this node.
        let ins = json!({"bundle": peer_bundle, "checksum": "00000000-00000000-00000000-00000000"});
        let (_, v) = route(
            &post("/v1/setup/bundle/inspect"),
            ins.to_string().as_bytes(),
            &ctx,
        )
        .await;
        assert_eq!(v["checksum_match"], false);
        let ins = json!({"bundle": peer_bundle, "checksum": peer_init.checksum});
        let (_, v) = route(
            &post("/v1/setup/bundle/inspect"),
            ins.to_string().as_bytes(),
            &ctx,
        )
        .await;
        assert_eq!(
            (v["checksum_match"].clone(), v["matches_this_node"].clone()),
            (json!(true), json!(true)),
            "{v}"
        );
        assert!(!conf.join("pair1.json").exists(), "inspect changes nothing");

        // A wrong checksum at join is refused by the helper and writes no configuration.
        let join_bad = json!({"stage": "join", "confirm": true, "idempotency_key": "pair-join-0",
            "plan": plan, "bundle": peer_bundle, "checksum": "00000000-00000000-00000000-00000000"});
        let (code, rc) = route(
            &post("/v1/setup/apply"),
            join_bad.to_string().as_bytes(),
            &ctx,
        )
        .await;
        assert_ne!(code, 200, "{rc}");
        assert!(!conf.join("pair1.json").exists());

        let join = json!({"stage": "join", "confirm": true, "idempotency_key": "pair-join-1",
            "plan": plan, "bundle": peer_bundle, "checksum": peer_init.checksum});
        let (code, rc) = route(&post("/v1/setup/apply"), join.to_string().as_bytes(), &ctx).await;
        assert_eq!(code, 200, "{rc}");
        assert!(conf.join("pair1.json").is_file());
        let (_, st) = route(&get("/v1/status"), b"", &ctx).await;
        assert_eq!(st["pairing"]["joined"], true);
        let audit = std::fs::read_to_string(base.join("state").join(AUDIT_FILE)).unwrap();
        assert!(audit.contains("setup/init") && audit.contains("setup/join"));
        assert!(!audit.contains(TOKEN));
        let _ = std::fs::remove_dir_all(&base);
    }
}
