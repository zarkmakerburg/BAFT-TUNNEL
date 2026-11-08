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
    serve(
        listener,
        Arc::new(Ctx {
            config,
            token,
            state_dir,
            lock: Mutex::new(()),
        }),
    )
    .await
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
    let (status, out) = route(&head, body, ctx).await;
    respond(&mut stream, status, &out).await
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
    if !matches!(path, "/v1/status" | "/v1/desired") {
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
        ("GET", "/v1/desired") => get_desired(ctx),
        ("PUT", "/v1/desired") => put_desired(ctx, body).await,
        _ => (405, json!({"error": "method not allowed"})),
    }
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
            Arc::new(Ctx {
                config: sample(),
                token: TOKEN.into(),
                state_dir: dir.map(Path::to_path_buf),
                lock: Mutex::new(()),
            }),
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
