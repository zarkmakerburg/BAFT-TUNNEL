//! Thin loopback client for ADR-0057's `POST /v1/policy/apply`.
//!
//! It performs no validation of its own: the agent and the root helper own every rule, so there is
//! exactly one interpretation of a request. Its only local checks are the ones that keep the token
//! off the network — the URL must be a plain loopback origin — and the receipt is printed as the
//! agent returned it.

use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

pub const DEFAULT_AGENT: &str = "http://127.0.0.1:18080";
/// Where the installed agent's token file lives (`scripts/install-agent-control.sh`).
pub const DEFAULT_TOKEN: &str = "/var/lib/baft-tunnel-agent/token";
const PATH: &str = "/v1/policy/apply";
const MAX_RESPONSE_BYTES: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Enable,
    Disable,
    Select,
    Reset,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Select => "select",
            Self::Reset => "reset",
        }
    }
}

/// A resolved loopback destination: every address the name resolves to is loopback, so the bearer
/// token cannot leave the host through a resolver answer (an `A` record for `localhost`, say).
#[derive(Debug)]
pub struct Target {
    /// `host:port` as the operator wrote it, for messages and the `Host` header.
    pub display: String,
    pub addrs: Vec<SocketAddr>,
}

/// Refuse anything that is not a loopback agent, and resolve the name here rather than letting the
/// connector resolve it later: `localhost` is only accepted when every address it resolves to is
/// loopback.
pub fn loopback_target(url: &str) -> Result<Target> {
    let rest = url.strip_prefix("http://").with_context(|| {
        format!("--agent must be http://127.0.0.1:PORT (got {url}); the agent API is loopback only")
    })?;
    if rest.contains('/') || rest.contains('@') || rest.contains('?') || rest.contains('#') {
        bail!("--agent must be a plain origin such as http://127.0.0.1:18080, with no path");
    }
    let (host, port) = rest
        .rsplit_once(':')
        .with_context(|| format!("--agent must name a port (got {url})"))?;
    let port: u16 = port
        .parse()
        .with_context(|| format!("--agent has an invalid port (got {url})"))?;
    if port == 0 {
        bail!("--agent must name a port other than 0");
    }
    let host = match host {
        "127.0.0.1" => "127.0.0.1".to_string(),
        "localhost" => "localhost".to_string(),
        "[::1]" => "[::1]".to_string(),
        other => bail!(
            "--agent must be a loopback address (127.0.0.1, localhost or [::1]); refusing {other}"
        ),
    };
    let display = format!("{host}:{port}");
    let addrs: Vec<SocketAddr> = display
        .to_socket_addrs()
        .with_context(|| format!("resolve the loopback agent address {display}"))?
        .collect();
    if addrs.is_empty() {
        bail!("the loopback agent address {display} did not resolve to any address");
    }
    if let Some(remote) = addrs.iter().find(|a| !a.ip().is_loopback()) {
        bail!(
            "{display} resolves to {remote}, which is not loopback; refusing to send the agent \
             token off this host"
        );
    }
    Ok(Target { display, addrs })
}

/// One request, built with the same fields the agent's typed route parses.
pub fn request_body(action: Action, profile: Option<&str>, key: &str) -> Result<Value> {
    match (action, profile) {
        (Action::Select, Some(id)) if !id.is_empty() => Ok(json!({
            "confirm": true, "idempotency_key": key, "action": "select", "profile": id,
        })),
        (Action::Select, _) => bail!("select needs a profile id"),
        (_, Some(_)) => bail!("{} takes no profile", action.as_str()),
        (_, None) => Ok(json!({
            "confirm": true, "idempotency_key": key, "action": action.as_str(),
        })),
    }
}

/// A fresh key per invocation: re-running the same command is a new request and converges to
/// `unchanged` (ADR-0057 decision 5) instead of replaying a stored receipt.
pub fn new_key() -> String {
    format!("cli-{}", hex::encode(rand::random::<[u8; 16]>()))
}

fn parse_response(raw: &[u8]) -> Result<(u16, Value)> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .context("the agent did not return an HTTP response")?;
    let code: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .context("the agent's HTTP status line is not readable")?;
    let value: Value = serde_json::from_str(body)
        .with_context(|| format!("the agent's answer is not JSON: {body}"))?;
    Ok((code, value))
}

/// One HTTP exchange with the loopback agent. The body is JSON when given.
async fn exchange(
    target: &Target,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&str>,
) -> Result<(u16, Value)> {
    let mut last = None;
    let mut stream = None;
    for addr in &target.addrs {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(error) => last = Some((*addr, error)),
        }
    }
    let Some(mut stream) = stream else {
        let (addr, error) = last.context("the agent address has no usable loopback address")?;
        return Err(error).with_context(|| format!("connect to the agent at {addr}"));
    };
    let display = &target.display;
    let body = body.unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {display}\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;
    let mut raw = Vec::new();
    stream
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut raw)
        .await?;
    if raw.len() as u64 > MAX_RESPONSE_BYTES {
        bail!("the agent's answer is larger than {MAX_RESPONSE_BYTES} bytes");
    }
    parse_response(&raw)
}

/// The instance this agent is bound to, from the existing host-tunnel list. `--instance` names
/// what the operator means, so a command for another instance must not write this one.
async fn bound_instance(target: &Target, token: &str) -> Result<String> {
    let (code, value) = exchange(target, "GET", "/v1/host/tunnels", token, None).await?;
    if code != 200 {
        bail!("the agent did not answer the host-tunnel list with 200 ({code})");
    }
    value["tunnels"]
        .as_array()
        .and_then(|list| {
            list.iter()
                .find(|t| t["current"] == json!(true))
                .and_then(|t| t["instance"].as_str())
        })
        .map(str::to_string)
        .context("the agent does not report a bound instance; start it with --instance")
}

/// Send the request to the agent and return its receipt, or its refusal reason verbatim.
pub async fn apply(
    agent: &str,
    token_file: &Path,
    instance: &str,
    action: Action,
    profile: Option<&str>,
) -> Result<Value> {
    let target = loopback_target(agent)?;
    let token = read_token(token_file)?;
    let bound = bound_instance(&target, &token).await?;
    if bound != instance {
        bail!(
            "the agent at {} manages {bound}, not {instance}; connect to the agent of the \
             instance you named, or use --agent",
            target.display
        );
    }
    let body = request_body(action, profile, &new_key())?.to_string();
    let (code, value) = exchange(&target, "POST", PATH, &token, Some(&body)).await?;
    if code == 200 {
        return Ok(value);
    }
    let reason = value["error"]
        .as_str()
        .or(value["reason"].as_str())
        .or(value["detail"].as_str())
        .unwrap_or("no reason given");
    let cause = value["cause"]
        .as_str()
        .map(|c| format!(" ({c})"))
        .unwrap_or_default();
    bail!(
        "the agent refused the request with {code} {}{cause}: {reason}{}",
        value["result"].as_str().unwrap_or("error"),
        match value["operator_action"].as_str() {
            Some(next) => format!("\nnext: {next}"),
            None => String::new(),
        }
    )
}

fn read_token(path: &Path) -> Result<String> {
    let token = std::fs::read_to_string(path)
        .with_context(|| {
            format!(
                "read the agent token from {} (pass --token-file)",
                path.display()
            )
        })?
        .trim()
        .to_string();
    if token.is_empty() {
        bail!("the agent token file {} is empty", path.display());
    }
    Ok(token)
}

/// ADR-0047's read: `GET /v1/status`'s `policy` member, unchanged, after confirming the agent is
/// bound to the instance the operator asked about. An `unknown` reading is returned, not an error:
/// only a read that failed to happen is an error (the caller exits 2 for it).
pub async fn status(agent: &str, token_path: &Path, instance: &str) -> Result<Value> {
    let target = loopback_target(agent)?;
    let token = read_token(token_path)?;
    let bound = bound_instance(&target, &token).await?;
    if bound != instance {
        bail!("the agent is bound to instance {bound}, not {instance}");
    }
    let (code, value) = exchange(&target, "GET", "/v1/status", &token, None).await?;
    if code != 200 {
        bail!("the agent did not answer status with 200 ({code})");
    }
    value
        .get("policy")
        .cloned()
        .context("the agent's status has no policy member (an agent older than this command?)")
}

/// Text form of the policy member. Null means "not observed", never zero: a metric without a
/// value prints its reason, and a state without evidence prints its reason too.
pub fn render_status(policy: &Value) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "policy: {}",
        policy["state"].as_str().unwrap_or("unknown")
    ));
    if let Some(reason) = policy["reason"].as_str() {
        out.push_str(&format!(" ({reason})"));
    }
    out.push('\n');
    out.push_str(&format!(
        "  feature compiled: {}\n",
        if policy["feature_compiled"] == json!(true) {
            "yes"
        } else {
            "no"
        }
    ));
    let switch = &policy["switch"];
    out.push_str(&format!(
        "  switch: {} ({})\n",
        switch["state"].as_str().unwrap_or("unknown"),
        switch["value"]
            .as_str()
            .or_else(|| switch["scan"].as_str())
            .unwrap_or("not set in a readable source")
    ));
    if let Some(sources) = switch["sources"].as_array().filter(|s| !s.is_empty()) {
        let sources: Vec<&str> = sources.iter().filter_map(Value::as_str).collect();
        out.push_str(&format!("    source: {}\n", sources.join(", ")));
    }
    let profile = &policy["profile"];
    out.push_str(&format!(
        "  profile: configured {} (effective {}), running {}\n",
        profile["configured"].as_str().unwrap_or("none"),
        profile["configured_effective"].as_str().unwrap_or("-"),
        profile["running"].as_str().unwrap_or("unknown"),
    ));
    let adaptive = &policy["adaptive"];
    out.push_str(&format!(
        "  adaptive: configured {}, effective {}\n",
        on_off(adaptive["configured"].as_bool()),
        on_off(adaptive["effective"].as_bool()),
    ));
    match policy["fallback"].as_object() {
        Some(fallback) => out.push_str(&format!(
            "  fallback: {}\n",
            fallback["cause"].as_str().unwrap_or("unknown")
        )),
        None => out.push_str("  fallback: none\n"),
    }
    if let Some(tuples) = policy["tuples"].as_array() {
        out.push_str("  tuples:\n");
        for row in tuples {
            out.push_str(&format!(
                "    {}/{}/{}/{}: {}\n",
                row["engine"].as_str().unwrap_or("-"),
                row["carrier"].as_str().unwrap_or("-"),
                row["profile"].as_str().unwrap_or("-"),
                row["claim"].as_str().unwrap_or("-"),
                row["evidence"].as_str().unwrap_or("-"),
            ));
        }
    }
    if let Some(metrics) = policy["metrics"].as_object() {
        out.push_str("  metrics:\n");
        for (name, metric) in metrics {
            out.push_str(&format!(
                "    {name}: {}\n",
                match metric["value"].as_u64() {
                    Some(value) => value.to_string(),
                    None => format!(
                        "unavailable ({})",
                        metric["reason"].as_str().unwrap_or("not_observed")
                    ),
                }
            ));
        }
    }
    out
}

fn on_off(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "on",
        Some(false) => "off",
        None => "unknown",
    }
}

/// Text form of a receipt: what was written, what it changed from, and what is verified. It never
/// claims the pack is active — that is ADR-0047's read, not this write's receipt.
pub fn render(receipt: &Value) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "policy {} {}: {}\n",
        receipt["action"].as_str().unwrap_or("-"),
        receipt["instance"].as_str().unwrap_or("-"),
        receipt["result"].as_str().unwrap_or("unknown"),
    ));
    if let Some(reason) = receipt["reason"].as_str() {
        out.push_str(&format!("  reason: {reason}\n"));
    }
    if let Some(cause) = receipt["cause"].as_str() {
        out.push_str(&format!("  cause: {cause}\n"));
    }
    for key in ["previous", "requested"] {
        if !receipt[key].is_null() {
            out.push_str(&format!("  {key}: {}\n", receipt[key]));
        }
    }
    if let Some(files) = receipt["files"].as_array().filter(|f| !f.is_empty()) {
        let names: Vec<&str> = files.iter().filter_map(Value::as_str).collect();
        out.push_str(&format!("  files: {}\n", names.join(", ")));
    }
    if let Some(verification) = receipt["verification"]["reason"].as_str() {
        out.push_str(&format!(
            "  verification ({}): {verification}\n",
            receipt["verification"]["level"].as_str().unwrap_or("none")
        ));
    }
    if receipt["restart_required"] == json!(true) {
        out.push_str("  restart required before the change takes effect\n");
    }
    if let Some(next) = receipt["operator_action"].as_str() {
        out.push_str(&format!("  next: {next}\n"));
    }
    if let Some(next) = receipt["next_commands"].as_array() {
        for command in next.iter().filter_map(Value::as_str) {
            out.push_str(&format!("  next: {command}\n"));
        }
    }
    if receipt["receipt_stored"] == json!(false) {
        out.push_str("  warning: the receipt could not be stored\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_loopback_origin_is_accepted() {
        let v4 = loopback_target("http://127.0.0.1:18080").unwrap();
        assert_eq!(v4.display, "127.0.0.1:18080");
        assert_eq!(v4.addrs, vec!["127.0.0.1:18080".parse().unwrap()]);
        let name = loopback_target("http://localhost:9000").unwrap();
        assert_eq!(name.display, "localhost:9000");
        assert!(!name.addrs.is_empty(), "localhost must resolve here");
        assert!(name.addrs.iter().all(|a| a.ip().is_loopback()), "{name:?}");
        let v6 = loopback_target("http://[::1]:18080").unwrap();
        assert_eq!(v6.display, "[::1]:18080");
        assert!(v6.addrs.iter().all(|a| a.ip().is_loopback()), "{v6:?}");
        for bad in [
            "https://127.0.0.1:18080",
            "http://198.51.100.7:18080",
            "http://example.com:18080",
            "http://127.0.0.1:18080/v1/policy/apply",
            "http://127.0.0.1",
            "http://127.0.0.1:0",
            "http://user@127.0.0.1:18080",
        ] {
            assert!(loopback_target(bad).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn the_body_carries_the_action_and_a_profile_only_for_select() {
        let enable = request_body(Action::Enable, None, "cli-abc").unwrap();
        assert_eq!(enable["action"], "enable");
        assert_eq!(enable["confirm"], true);
        assert!(enable.get("profile").is_none());
        let select = request_body(Action::Select, Some("gaming/1"), "cli-abc").unwrap();
        assert_eq!(select["profile"], "gaming/1");
        assert!(request_body(Action::Select, None, "cli-abc").is_err());
        assert!(request_body(Action::Select, Some(""), "cli-abc").is_err());
        assert!(request_body(Action::Reset, Some("gaming/1"), "cli-abc").is_err());
        let key = new_key();
        assert!(crate::apply::valid_key(&key), "{key}");
        assert_ne!(key, new_key());
    }

    #[test]
    fn a_receipt_is_rendered_without_claiming_the_pack_is_active() {
        let receipt = json!({
            "action": "enable", "instance": "edge", "result": "configured",
            "previous": {"switch": null, "profile": null, "adaptive": false},
            "requested": {"switch": "sched", "profile": null, "adaptive": false},
            "files": ["edge.json", "50-policy-pack.conf"],
            "restart_required": false,
            "verification": {"level": "unit", "reason": "the unit was restarted"},
            "next_commands": ["systemctl status baft-tunnel@edge"],
        });
        let text = render(&receipt);
        assert!(text.contains("policy enable edge: configured"), "{text}");
        assert!(text.contains("verification (unit)"), "{text}");
        assert!(text.contains("50-policy-pack.conf"), "{text}");
        assert!(!text.contains("active"), "{text}");
    }

    #[test]
    fn a_status_reading_prints_reasons_instead_of_inventing_values() {
        let policy = json!({
            "state": "unknown", "reason": "no_running_evidence", "feature_compiled": true,
            "switch": {"state": "on", "value": "sched", "sources": ["/etc/systemd/system/baft-tunnel@edge.service.d/50-policy-pack.conf"], "scan": null},
            "profile": {"configured": "gaming/1", "configured_effective": "gaming/1", "running": null},
            "adaptive": {"configured": false, "effective": null},
            "fallback": null,
            "tuples": [{"engine": "tcp-tls", "carrier": "tcp", "profile": "gaming/1", "claim": "unknown", "evidence": null}],
            "metrics": {
                "guard_violations": {"value": null, "reason": "not_instrumented"},
                "queueing_delay_estimate_ms": {"value": null, "reason": "signal_blind"},
            },
        });
        let text = render_status(&policy);
        assert!(
            text.contains("policy: unknown (no_running_evidence)"),
            "{text}"
        );
        assert!(text.contains("configured gaming/1"), "{text}");
        assert!(text.contains("running unknown"), "{text}");
        assert!(text.contains("tcp-tls/tcp/gaming/1/unknown"), "{text}");
        assert!(
            text.contains("guard_violations: unavailable (not_instrumented)"),
            "{text}"
        );
        assert!(!text.contains(": 0"), "{text}");
    }

    #[test]
    fn an_error_answer_keeps_the_agent_status_and_cause() {
        let raw = b"HTTP/1.1 409 Conflict\r\nContent-Length: 60\r\n\r\n\
            {\"error\":\"x\",\"result\":\"refused\",\"cause\":\"switch_managed_elsewhere\"}";
        let (code, value) = parse_response(raw).unwrap();
        assert_eq!(code, 409);
        assert_eq!(value["cause"], "switch_managed_elsewhere");
        assert!(parse_response(b"not http").is_err());
    }
}
