//! Local read-only status API (P3-002 slice A1).
//!
//! Loopback only, bearer-token protected, GET only. It reports the redacted support report plus a
//! liveness probe of the configured tunnel listener. It never starts, stops or changes a tunnel.

use std::{
    net::SocketAddr,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use rand::RngCore;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time::timeout,
};

use crate::config::RuntimeConfig;

const MIN_TOKEN_LEN: usize = 32;
const MAX_REQUEST_BYTES: usize = 8 * 1024;
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
    use std::io::Write;
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

pub async fn run(config: RuntimeConfig, listen: SocketAddr, token: String) -> Result<()> {
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("cannot bind agent API on {listen}"))?;
    eprintln!("Agent API listening on {listen} (read-only; token required for /v1/status)");
    serve(listener, Arc::new(config), Arc::new(token)).await
}

pub async fn serve(
    listener: TcpListener,
    config: Arc<RuntimeConfig>,
    token: Arc<String>,
) -> Result<()> {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (stream, _) = listener.accept().await?;
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let (config, token) = (config.clone(), token.clone());
        tokio::spawn(async move {
            let _ = timeout(IO_TIMEOUT * 2, handle(stream, &config, &token)).await;
            drop(permit);
        });
    }
}

async fn handle(mut stream: TcpStream, config: &RuntimeConfig, token: &str) -> Result<()> {
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 512];
    loop {
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() >= MAX_REQUEST_BYTES {
            return respond(&mut stream, 431, &json!({"error": "request too large"})).await;
        }
        let n = timeout(IO_TIMEOUT, stream.read(&mut chunk)).await??;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let text = String::from_utf8_lossy(&buf);
    let (status, body) = route(&text, config, token).await;
    respond(&mut stream, status, &body).await
}

async fn route(request: &str, config: &RuntimeConfig, token: &str) -> (u16, Value) {
    let mut lines = request.split("\r\n");
    let mut parts = lines.next().unwrap_or_default().split(' ');
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if method != "GET" {
        return (405, json!({"error": "only GET is supported"}));
    }
    match path {
        "/v1/health" => (200, json!({"ok": true})),
        "/v1/status" => {
            let presented = lines
                .filter_map(|l| l.split_once(':'))
                .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                .and_then(|(_, v)| v.trim().strip_prefix("Bearer "))
                .unwrap_or("");
            if !constant_time_eq(presented.as_bytes(), token.as_bytes()) {
                return (401, json!({"error": "unauthorized"}));
            }
            (200, status(config).await)
        }
        _ => (404, json!({"error": "not found"})),
    }
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

    fn sample() -> RuntimeConfig {
        serde_json::from_value(json!({
            "mode": "forward", "listen": "127.0.0.1:1", "remote": "192.0.2.1:8443",
            "server_name": "x", "local_id": "a", "peer_id": "b", "tunnel_id": "t",
            "direction": "direct", "peer_fingerprint": "00", "key": "/secret/key.pem"
        }))
        .unwrap()
    }

    async fn get(path: &str, auth: Option<&str>) -> (u16, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(serve(
            listener,
            Arc::new(sample()),
            Arc::new("t".repeat(40)),
        ));
        let mut s = TcpStream::connect(addr).await.unwrap();
        let mut req = format!("GET {path} HTTP/1.1\r\nHost: x\r\n");
        if let Some(a) = auth {
            req += &format!("Authorization: Bearer {a}\r\n");
        }
        s.write_all((req + "\r\n").as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        task.abort();
        let code = out[9..12].parse().unwrap();
        (code, out)
    }

    #[tokio::test]
    async fn status_requires_token_and_redacts() {
        assert_eq!(get("/v1/status", None).await.0, 401);
        assert_eq!(get("/v1/status", Some("wrong")).await.0, 401);
        let (code, body) = get("/v1/status", Some(&"t".repeat(40))).await;
        assert_eq!(code, 200);
        assert!(body.contains("\"listener_accepting\":false"));
        assert!(!body.contains("/secret"));
        assert!(!body.contains("192.0.2.1"));
    }

    #[tokio::test]
    async fn health_open_and_other_routes_rejected() {
        assert_eq!(get("/v1/health", None).await.0, 200);
        assert_eq!(get("/v1/other", None).await.0, 404);
    }

    #[test]
    fn listen_must_be_loopback() {
        assert!(parse_listen("127.0.0.1:9000").is_ok());
        assert!(parse_listen("0.0.0.0:9000").is_err());
        assert!(parse_listen("127.0.0.1:0").is_err());
    }

    #[test]
    fn token_is_private_never_overwritten_and_checked() {
        let dir = std::env::temp_dir().join(format!("baft-agent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("token");
        write_token(&p).unwrap();
        assert!(write_token(&p).is_err());
        assert_eq!(read_token(&p).unwrap().len(), 64);
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_token(&p).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
