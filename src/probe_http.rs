//! ADR-0052: consent-gated, bounded HTTPS client for the external probe providers.
//!
//! Fixed allow-list of two hosts, no redirects, no cookies, no credentials, size and
//! time caps. The target IP is checked with `target_guard` before anything is sent, and
//! nothing is sent unless the operator enabled that provider (`Consent`).
//! The stream logic is generic so it is tested without a network.
#![allow(dead_code)] // wired by the panel endpoint in a later slice

use crate::probe_adapters::{self, Provider, Row};
use crate::target_guard::{self, Gate, LookupGate};
use anyhow::{Result, bail};
use serde_json::Value;
use std::net::IpAddr;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_RESPONSE: usize = 256 * 1024;
pub const IO_TIMEOUT: Duration = Duration::from_secs(10);
const CHECKHOST: &str = "check-host.net";
const GLOBALPING: &str = "api.globalping.io";

/// Per-installation switches. Default: everything off (ADR-0052).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Consent {
    pub checkhost: bool,
    pub globalping: bool,
}

impl Consent {
    pub fn allows(&self, p: Provider) -> bool {
        match p {
            Provider::CheckHost => self.checkhost,
            Provider::Globalping => self.globalping,
        }
    }
}

/// Build a request. Only the two fixed hosts are accepted; the caller cannot choose
/// another host, add headers or change the method beyond GET and JSON POST.
pub fn build_request(
    host: &str,
    method: &str,
    path_query: &str,
    json_body: Option<&str>,
) -> Result<Vec<u8>> {
    if host != CHECKHOST && host != GLOBALPING {
        bail!("host not allowed");
    }
    if method != "GET" && method != "POST" {
        bail!("method not allowed");
    }
    if !path_query.starts_with('/') || path_query.bytes().any(|b| b <= b' ' || b == 0x7f) {
        bail!("bad path");
    }
    let mut r = format!(
        "{method} {path_query} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nUser-Agent: baft-tunnel-probe\r\nConnection: close\r\n"
    );
    if let Some(b) = json_body {
        r.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    r.push_str("\r\n");
    if let Some(b) = json_body {
        r.push_str(b);
    }
    Ok(r.into_bytes())
}

/// Parse a full HTTP/1.1 response (read until close) into (status, JSON body).
/// Anything malformed, oversize or not JSON is an error and becomes Unknown upstream.
pub fn parse_response(raw: &[u8]) -> Result<(u16, Value)> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n");
    let Some(i) = split else {
        bail!("no header end")
    };
    let head = std::str::from_utf8(&raw[..i])?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.split(' ');
    if !parts.next().is_some_and(|v| v.starts_with("HTTP/1.")) {
        bail!("bad status line");
    }
    let status: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("bad status"))?;
    let chunked = lines.any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let body = &raw[i + 4..];
    let body = if chunked {
        dechunk(body)?
    } else {
        body.to_vec()
    };
    let v: Value = serde_json::from_slice(&body)?;
    Ok((status, v))
}

fn dechunk(mut b: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let Some(p) = b.windows(2).position(|w| w == b"\r\n") else {
            bail!("bad chunk")
        };
        let size_txt = std::str::from_utf8(&b[..p])?
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        let size = usize::from_str_radix(size_txt, 16)?;
        b = &b[p + 2..];
        if size == 0 {
            return Ok(out);
        }
        if b.len() < size + 2 || out.len() + size > MAX_RESPONSE {
            bail!("bad chunk size");
        }
        out.extend_from_slice(&b[..size]);
        b = &b[size + 2..];
    }
}

/// Send one request over any stream and read the bounded response.
pub async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut S,
    req: &[u8],
) -> Result<(u16, Value)> {
    tokio::time::timeout(IO_TIMEOUT, async {
        s.write_all(req).await?;
        let mut raw = Vec::new();
        let mut limited = s.take(MAX_RESPONSE as u64 + 1);
        limited.read_to_end(&mut raw).await?;
        if raw.len() > MAX_RESPONSE {
            bail!("response too large");
        }
        parse_response(&raw)
    })
    .await?
}

/// TLS connector using the system CA bundle (no new dependency). Fails closed when the
/// bundle is missing or empty.
pub fn system_connector() -> Result<tokio_rustls::TlsConnector> {
    let pem = std::fs::read("/etc/ssl/certs/ca-certificates.crt")?;
    let mut roots = rustls::RootCertStore::empty();
    for p in x509_parser::pem::Pem::iter_from_buffer(&pem).flatten() {
        let _ = roots.add(rustls::pki_types::CertificateDer::from(p.contents));
    }
    if roots.is_empty() {
        bail!("no system CA certificates");
    }
    let cfg = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(tokio_rustls::TlsConnector::from(std::sync::Arc::new(cfg)))
}

async fn https(
    host: &'static str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<(u16, Value)> {
    let req = build_request(host, method, path, body)?;
    let tcp =
        tokio::time::timeout(IO_TIMEOUT, tokio::net::TcpStream::connect((host, 443))).await??;
    let name = rustls::pki_types::ServerName::try_from(host)?;
    let mut tls =
        tokio::time::timeout(IO_TIMEOUT, system_connector()?.connect(name, tcp)).await??;
    exchange(&mut tls, &req).await
}

/// Result of one provider for the panel: rows (possibly empty) and a status text.
#[derive(Debug, PartialEq)]
pub struct Report {
    pub provider: Provider,
    pub rows: Vec<Row>,
    pub note: &'static str,
}

fn unknown(provider: Provider, note: &'static str) -> Report {
    Report {
        provider,
        rows: Vec::new(),
        note,
    }
}

/// Run the check-host TCP check from Iran nodes. Every failure is Unknown, never zero.
pub async fn run_checkhost(
    ip: IpAddr,
    port: u16,
    consent: Consent,
    gate: &mut LookupGate,
    now: u64,
) -> Report {
    let p = Provider::CheckHost;
    if !consent.allows(p) {
        return unknown(p, "disabled");
    }
    let Ok(ip) = target_guard::check_public(ip) else {
        return unknown(p, "target not public");
    };
    if gate.ask(ip, now) == Gate::Limited {
        return unknown(p, "rate limited here");
    }
    let Ok((200, hosts)) = https(CHECKHOST, "GET", "/nodes/hosts", None).await else {
        return unknown(p, "provider unreachable");
    };
    let nodes: Vec<String> = probe_adapters::checkhost_iran_nodes(&hosts)
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    let Some(url) = probe_adapters::checkhost_tcp_url(ip, port, &nodes) else {
        return unknown(p, "no Iran nodes listed");
    };
    let path = url.trim_start_matches("https://check-host.net");
    let Ok((200, start)) = https(CHECKHOST, "GET", path, None).await else {
        return unknown(p, "provider unreachable");
    };
    if start.get("error").is_some() {
        return unknown(p, "provider rate limit");
    }
    let (Some(id), Some(nodes_map)) = (
        start.get("request_id").and_then(Value::as_str),
        start.get("nodes"),
    ) else {
        return unknown(p, "unexpected reply");
    };
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return unknown(p, "unexpected reply");
    }
    let mut rows = Vec::new();
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let Ok((200, res)) = https(CHECKHOST, "GET", &format!("/check-result/{id}"), None).await
        else {
            continue;
        };
        rows = probe_adapters::parse_checkhost_tcp(nodes_map, &res);
        if rows
            .iter()
            .all(|r| r.outcome != probe_adapters::Outcome::Unknown)
        {
            break;
        }
    }
    Report {
        provider: p,
        rows,
        note: "ok",
    }
}

/// Run the Globalping ICMP check from Iran probes. ICMP only (no TCP type), so a failure
/// is not proof the service is down. Every failure is Unknown, never zero.
pub async fn run_globalping(
    ip: IpAddr,
    consent: Consent,
    gate: &mut LookupGate,
    now: u64,
) -> Report {
    let p = Provider::Globalping;
    if !consent.allows(p) {
        return unknown(p, "disabled");
    }
    let Ok(ip) = target_guard::check_public(ip) else {
        return unknown(p, "target not public");
    };
    if gate.ask(ip, now) == Gate::Limited {
        return unknown(p, "rate limited here");
    }
    let body = probe_adapters::globalping_ping_body(ip, 8).to_string();
    let Ok((code, start)) = https(GLOBALPING, "POST", "/v1/measurements", Some(&body)).await else {
        return unknown(p, "provider unreachable");
    };
    if code != 200 && code != 201 && code != 202 {
        return unknown(p, "provider rate limit");
    }
    let Some(id) = start.get("id").and_then(Value::as_str) else {
        return unknown(p, "unexpected reply");
    };
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return unknown(p, "unexpected reply");
    }
    let mut rows = Vec::new();
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let Ok((200, m)) = https(GLOBALPING, "GET", &format!("/v1/measurements/{id}"), None).await
        else {
            continue;
        };
        rows = probe_adapters::parse_globalping_ping(&m);
        if m.get("status").and_then(Value::as_str) == Some("finished") {
            break;
        }
    }
    Report {
        provider: p,
        rows,
        note: "icmp only",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[test]
    fn request_is_restricted() {
        assert!(build_request("evil.com", "GET", "/", None).is_err());
        assert!(build_request(CHECKHOST, "DELETE", "/", None).is_err());
        assert!(build_request(CHECKHOST, "GET", "/a b", None).is_err());
        assert!(build_request(CHECKHOST, "GET", "/a\r\nX: 1", None).is_err());
        assert!(build_request(CHECKHOST, "GET", "x", None).is_err());
        let r = String::from_utf8(
            build_request(GLOBALPING, "POST", "/v1/measurements", Some("{}")).unwrap(),
        )
        .unwrap();
        assert!(r.starts_with("POST /v1/measurements HTTP/1.1\r\nHost: api.globalping.io\r\n"));
        assert!(r.contains("Content-Length: 2") && r.ends_with("\r\n\r\n{}"));
    }

    #[test]
    fn parses_plain_chunked_and_rejects_garbage() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}";
        assert_eq!(
            parse_response(plain).unwrap(),
            (200, serde_json::json!({"a":1}))
        );
        let chunked = b"HTTP/1.1 202 Accepted\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"a\r\n4\r\n\":1}\r\n0\r\n\r\n";
        assert_eq!(
            parse_response(chunked).unwrap(),
            (202, serde_json::json!({"a":1}))
        );
        for bad in [
            &b"garbage"[..],
            b"HTTP/1.1 200 OK\r\n\r\nnot json",
            b"HTTP/9 200 OK\r\n\r\n{}",
            b"HTTP/1.1 x\r\n\r\n{}",
        ] {
            assert!(parse_response(bad).is_err());
        }
        let evil = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffffffff\r\nabc";
        assert!(parse_response(evil).is_err());
    }

    #[tokio::test]
    async fn exchange_over_a_stream_and_size_cap() {
        let (mut c, mut s) = duplex(1 << 20);
        let srv = tokio::spawn(async move {
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf).await;
            s.write_all(b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":1}")
                .await
                .unwrap();
        });
        let req = build_request(CHECKHOST, "GET", "/nodes/hosts", None).unwrap();
        assert_eq!(
            exchange(&mut c, &req).await.unwrap(),
            (200, serde_json::json!({"ok":1}))
        );
        srv.await.unwrap();

        let (mut c, mut s) = duplex(1 << 20);
        tokio::spawn(async move {
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf).await;
            let big = vec![b'a'; MAX_RESPONSE + 10];
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await;
            let _ = s.write_all(&big).await;
        });
        assert!(exchange(&mut c, &req).await.is_err());
    }

    #[tokio::test]
    async fn nothing_is_sent_without_consent_or_for_private_targets() {
        let mut gate = LookupGate::new(600, 3, 10);
        let off = run_checkhost(
            "8.8.8.8".parse().unwrap(),
            443,
            Consent::default(),
            &mut gate,
            0,
        )
        .await;
        assert_eq!(off.note, "disabled");
        assert!(off.rows.is_empty());
        let on = Consent {
            checkhost: true,
            globalping: false,
        };
        let priv_ = run_checkhost("10.0.0.1".parse().unwrap(), 443, on, &mut gate, 0).await;
        assert_eq!(priv_.note, "target not public");
        let meta = run_checkhost("169.254.169.254".parse().unwrap(), 80, on, &mut gate, 0).await;
        assert_eq!(meta.note, "target not public");
    }

    /// Live smoke test against the real provider; run by hand:
    /// `cargo test --release live_checkhost -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_checkhost_nodes_over_real_tls() {
        let (st, v) = https(CHECKHOST, "GET", "/nodes/hosts", None).await.unwrap();
        assert_eq!(st, 200);
        let n = probe_adapters::checkhost_iran_nodes(&v);
        println!("{n:?}");
        assert!(!n.is_empty());
    }
}
