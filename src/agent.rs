//! Local read-only status API (P3-002 slice A1).
//!
//! Loopback only, bearer-token protected, GET only. It reports the redacted support report plus a
//! liveness probe of the configured tunnel listener. It never starts, stops or changes a tunnel.

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
const MAX_CONNECTIONS: usize = 16;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
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
        }
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
    }
}

pub async fn run(
    config: RuntimeConfig,
    listen: SocketAddr,
    token: String,
    state_dir: Option<PathBuf>,
) -> Result<()> {
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("cannot bind agent API on {listen}"))?;
    eprintln!("Agent API listening on {listen} (token required; never applies a config itself)");
    let ctx = Arc::new(Ctx::new(config, token, state_dir));
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
            let _ = timeout(IO_TIMEOUT * 2, handle(stream, &ctx)).await;
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
            | "/v1/history"
            | "/v1/security"
            | "/v1/audit"
            | "/v1/engines"
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
        ("GET", "/v1/status") => (200, status(&ctx.config).await),
        ("GET", "/v1/history") => (200, history(ctx)),
        ("GET", "/v1/security") => (200, security(&ctx.config)),
        ("GET", "/v1/audit") => get_audit(ctx),
        ("GET", "/v1/engines") => (200, engines()),
        ("GET", "/v1/desired") => get_desired(ctx),
        ("PUT", "/v1/desired") => put_desired(ctx, body).await,
        ("POST", "/v1/setup/plan") => setup_plan(body),
        ("POST", "/v1/setup/preflight") => preflight(body).await,
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
    json!({"overall": overall, "checks": checks, "applied": false, "apply_available": false})
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
        let _ = audit(dir, "rejected", &digest, &detail);
        return (
            422,
            json!({"error": "configuration rejected", "detail": detail}),
        );
    }
    match store(dir, body).and_then(|()| audit(dir, "accepted", &digest, "")) {
        Ok(()) => (
            200,
            json!({"accepted": true, "sha256": digest, "applied": false}),
        ),
        Err(error) => (500, json!({"error": format!("cannot store: {error:#}")})),
    }
}

fn store(dir: &Path, body: &[u8]) -> Result<()> {
    let tmp = dir.join(format!(".desired.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(body)?;
    file.sync_all()?;
    std::fs::rename(&tmp, dir.join(DESIRED_FILE))?;
    Ok(())
}

/// Append one audit line. Holds only a timestamp, the action, the body hash and a short reason.
fn audit(dir: &Path, result: &str, sha256: &str, detail: &str) -> Result<()> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(dir.join(AUDIT_FILE))?;
    let line = json!({"ts": ts, "action": "put_desired", "result": result, "sha256": sha256, "detail": detail});
    writeln!(file, "{line}")?;
    Ok(())
}

async fn status(config: &RuntimeConfig) -> Value {
    let listening = match config.listen_addr().parse::<SocketAddr>() {
        Ok(addr) => matches!(
            timeout(PROBE_TIMEOUT, TcpStream::connect(addr)).await,
            Ok(Ok(_))
        ),
        Err(_) => false,
    };
    json!({
        "agent": "baft-tunnel",
        "version": env!("CARGO_PKG_VERSION"),
        "engine": config.engine_name(),
        "config": config.support_report(),
        "listener_accepting": listening,
    })
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
        assert_eq!(v["apply_available"], false);
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
        audit(&dir, "accepted", "abc", "").unwrap();
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
