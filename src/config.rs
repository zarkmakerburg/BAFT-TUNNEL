use std::{
    fs::File,
    io::BufReader,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::{
    authorization::ConnectionDirection,
    handshake::{validate_fingerprint, validate_identity},
    tls, tunnel,
    tunnel::{ForwardOptions, ServerOptions},
};

const CRL_WARNING_SECONDS: i64 = 24 * 3600;

fn default_max_streams() -> usize {
    8
}

#[derive(Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RuntimeConfig {
    Serve {
        listen: String,
        target: String,
        local_id: String,
        peer_id: String,
        tunnel_id: String,
        direction: ConnectionDirection,
        peer_fingerprint: String,
        ca: PathBuf,
        crl: PathBuf,
        cert: PathBuf,
        key: Option<PathBuf>,
        #[serde(default)]
        key_credential: Option<String>,
        #[serde(default = "default_max_streams")]
        max_streams: usize,
    },
    Forward {
        listen: String,
        remote: String,
        server_name: String,
        local_id: String,
        peer_id: String,
        tunnel_id: String,
        direction: ConnectionDirection,
        peer_fingerprint: String,
        ca: PathBuf,
        crl: PathBuf,
        cert: PathBuf,
        key: Option<PathBuf>,
        #[serde(default)]
        key_credential: Option<String>,
        #[serde(default = "default_max_streams")]
        max_streams: usize,
        /// Allow the local forwarder to listen on a non-loopback address. Anyone who can reach
        /// that address can use the tunnel without authentication, so it must be explicit.
        #[serde(default)]
        allow_non_loopback_listen: bool,
    },
}

impl RuntimeConfig {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Serve {
                listen,
                target,
                local_id,
                peer_id,
                tunnel_id,
                direction: _,
                peer_fingerprint,
                ca,
                crl,
                cert,
                key,
                key_credential,
                max_streams,
            } => {
                validate_common(local_id, peer_id, tunnel_id, peer_fingerprint, *max_streams)?;
                parse_socket(listen, "listen")?;
                validate_target(parse_socket(target, "target")?)?;
                let key_path = private_key_path(key.as_deref(), key_credential.as_deref())?;
                tls::server_config(ca, crl, cert, &key_path)
                    .context("server TLS material is invalid")?;
            }
            Self::Forward {
                listen,
                remote,
                server_name,
                local_id,
                peer_id,
                tunnel_id,
                direction: _,
                peer_fingerprint,
                ca,
                crl,
                cert,
                key,
                key_credential,
                max_streams,
                allow_non_loopback_listen,
            } => {
                validate_common(local_id, peer_id, tunnel_id, peer_fingerprint, *max_streams)?;
                validate_forward_listen(
                    parse_socket(listen, "listen")?,
                    *allow_non_loopback_listen,
                )?;
                validate_target(parse_socket(remote, "remote")?)?;
                tls::server_name(server_name)?;
                let key_path = private_key_path(key.as_deref(), key_credential.as_deref())?;
                tls::client_config(ca, crl, cert, &key_path)
                    .context("client TLS material is invalid")?;
            }
        }
        self.warn_if_crl_expiring();
        Ok(())
    }

    fn crl_path(&self) -> &Path {
        match self {
            Self::Serve { crl, .. } | Self::Forward { crl, .. } => crl,
        }
    }

    /// Warn when the CRL is close to expiry; peers fail closed once it expires.
    fn warn_if_crl_expiring(&self) {
        match tls::crl_seconds_remaining(self.crl_path()) {
            Ok(remaining) if remaining <= 0 => eprintln!(
                "WARNING: certificate revocation list has expired; TLS connections will fail closed until it is refreshed"
            ),
            Ok(remaining) if remaining < CRL_WARNING_SECONDS => eprintln!(
                "WARNING: certificate revocation list expires in {} hours; refresh it before then",
                remaining / 3600
            ),
            Ok(_) => {}
            Err(error) => eprintln!("WARNING: could not determine CRL expiry: {error:#}"),
        }
    }

    pub fn plan(&self) -> String {
        match self {
            Self::Serve {
                listen,
                target,
                local_id,
                peer_id,
                tunnel_id,
                direction,
                max_streams,
                key,
                key_credential,
                ..
            } => format!(
                "Mode: serve\nDirection: {}\nListen: {listen}\nFixed destination: {target}\nLocal identity: {local_id}\nExpected peer: {peer_id}\nTunnel: {tunnel_id}\nMax streams: {max_streams}\nPrivate key source: {}\nSecurity: TLS 1.3 + mutual certificate auth + SHA-256 peer pin\nHost changes: none",
                direction.as_str(),
                key_source_description(key.as_ref(), key_credential.as_deref())
            ),
            Self::Forward {
                listen,
                remote,
                server_name,
                local_id,
                peer_id,
                tunnel_id,
                direction,
                max_streams,
                key,
                key_credential,
                ..
            } => format!(
                "Mode: forward\nDirection: {}\nLocal listener: {listen}\nRemote peer: {remote}\nTLS server name: {server_name}\nLocal identity: {local_id}\nExpected peer: {peer_id}\nTunnel: {tunnel_id}\nMax streams: {max_streams}\nPrivate key source: {}\nSecurity: TLS 1.3 + mutual certificate auth + SHA-256 peer pin\nHost changes: none",
                direction.as_str(),
                key_source_description(key.as_ref(), key_credential.as_deref())
            ),
        }
    }

    pub fn support_report(&self) -> serde_json::Value {
        let (
            mode,
            local_id,
            peer_id,
            tunnel_id,
            direction,
            peer_fingerprint,
            max_streams,
            ca,
            cert,
            key_credential,
        ) = match self {
            Self::Serve {
                local_id,
                peer_id,
                tunnel_id,
                direction,
                peer_fingerprint,
                max_streams,
                ca,
                cert,
                key_credential,
                ..
            } => (
                "serve",
                local_id,
                peer_id,
                tunnel_id,
                direction,
                peer_fingerprint,
                max_streams,
                ca,
                cert,
                key_credential,
            ),
            Self::Forward {
                local_id,
                peer_id,
                tunnel_id,
                direction,
                peer_fingerprint,
                max_streams,
                ca,
                cert,
                key_credential,
                ..
            } => (
                "forward",
                local_id,
                peer_id,
                tunnel_id,
                direction,
                peer_fingerprint,
                max_streams,
                ca,
                cert,
                key_credential,
            ),
        };
        serde_json::json!({
            "application": "baft-tunnel",
            "version": env!("CARGO_PKG_VERSION"),
            "runtime": {
                "os": std::env::consts::OS,
                "architecture": std::env::consts::ARCH
            },
            "configuration": {
                "mode": mode,
                "local_identity": local_id,
                "peer_identity": peer_id,
                "tunnel_id": tunnel_id,
                "direction": direction,
                "max_streams": max_streams,
                "peer_certificate_pin_configured": !peer_fingerprint.is_empty(),
                "ca_file_present": ca.is_file(),
                "certificate_file_present": cert.is_file(),
                "private_key_source": if key_credential.is_some() { "systemd-credential" } else { "file" },
                "private_key_credential_id": key_credential
            },
            "crl": {
                "seconds_until_expiry": tls::crl_seconds_remaining(self.crl_path()).ok(),
                "expiry_known": tls::crl_seconds_remaining(self.crl_path()).is_ok()
            },
            "security": {
                "tls_version": "1.3",
                "mutual_certificate_authentication": true,
                "private_key_contents_included": false,
                "certificate_contents_included": false,
                "network_endpoints_included": false
            },
            "environment": {
                "host_settings_modified": false,
                "network_probe_performed": false,
                "real_host_lab": "NOT_RUN"
            }
        })
    }

    pub async fn apply(self) -> Result<()> {
        match self {
            Self::Serve {
                listen,
                target,
                local_id,
                peer_id,
                tunnel_id,
                direction,
                peer_fingerprint,
                ca,
                crl,
                cert,
                key,
                key_credential,
                max_streams,
            } => {
                let key = private_key_path(key.as_deref(), key_credential.as_deref())?;
                tunnel::serve(ServerOptions {
                    listen,
                    target,
                    local_id,
                    peer_id,
                    tunnel_id,
                    direction,
                    peer_fingerprint,
                    ca,
                    crl,
                    cert,
                    key,
                    max_streams,
                })
                .await
            }
            Self::Forward {
                listen,
                remote,
                server_name,
                local_id,
                peer_id,
                tunnel_id,
                direction,
                peer_fingerprint,
                ca,
                crl,
                cert,
                key,
                key_credential,
                max_streams,
                allow_non_loopback_listen: _,
            } => {
                let key = private_key_path(key.as_deref(), key_credential.as_deref())?;
                tunnel::forward(ForwardOptions {
                    listen,
                    remote,
                    server_name,
                    local_id,
                    peer_id,
                    tunnel_id,
                    direction,
                    peer_fingerprint,
                    ca,
                    crl,
                    cert,
                    key,
                    max_streams,
                })
                .await
            }
        }
    }
}

fn private_key_path(key: Option<&Path>, credential_id: Option<&str>) -> Result<PathBuf> {
    match (key, credential_id) {
        (Some(path), None) => Ok(path.to_path_buf()),
        (None, Some(identifier)) => crate::key_custody::systemd_credential_path(identifier),
        (Some(_), Some(_)) => bail!("configure exactly one of key or key_credential"),
        (None, None) => bail!("configure exactly one of key or key_credential"),
    }
}

fn key_source_description(key: Option<&PathBuf>, credential_id: Option<&str>) -> String {
    match (key, credential_id) {
        (Some(_), None) => "filesystem (LAB compatibility)".to_owned(),
        (None, Some(identifier)) => format!("systemd credential ({identifier})"),
        _ => "invalid (configure exactly one source)".to_owned(),
    }
}

pub fn load(path: &Path) -> Result<RuntimeConfig> {
    let file = File::open(path).with_context(|| format!("open config file {}", path.display()))?;
    serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("parse JSON config {}", path.display()))
}

fn parse_socket(value: &str, field: &str) -> Result<SocketAddr> {
    value
        .parse()
        .with_context(|| format!("{field} must be an IP:port socket address"))
}

fn validate_forward_listen(address: SocketAddr, allow_non_loopback: bool) -> Result<()> {
    if address.port() == 0 {
        bail!("listen must have a non-zero port");
    }
    if !address.ip().is_loopback() && !allow_non_loopback {
        bail!(
            "forward listen address must be loopback; unauthenticated clients could use the tunnel. Set allow_non_loopback_listen (--allow-non-loopback-listen) to accept this risk"
        );
    }
    Ok(())
}

fn validate_target(address: SocketAddr) -> Result<()> {
    if address.port() == 0 || address.ip().is_unspecified() || address.ip().is_multicast() {
        bail!("target must be a specific unicast IP address with a non-zero port");
    }
    Ok(())
}

fn validate_common(
    local: &str,
    peer: &str,
    tunnel: &str,
    fingerprint: &str,
    streams: usize,
) -> Result<()> {
    validate_identity(local)?;
    validate_identity(peer)?;
    validate_identity(tunnel)?;
    validate_fingerprint(fingerprint)?;
    if local == peer {
        bail!("local and peer identities must differ");
    }
    if !(1..=8).contains(&streams) {
        bail!("max_streams must be between 1 and 8");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_stream_count_is_bounded() {
        let fingerprint = "a".repeat(64);
        assert!(validate_common("node-a", "node-b", "tunnel", &fingerprint, 1).is_ok());
        assert!(validate_common("node-a", "node-b", "tunnel", &fingerprint, 8).is_ok());
        assert!(validate_common("node-a", "node-b", "tunnel", &fingerprint, 9).is_err());
    }

    #[test]
    fn config_rejects_unknown_fields_and_defaults_stream_limit() {
        let valid = r#"{
            "mode":"serve","direction":"direct","listen":"127.0.0.1:9443","target":"127.0.0.1:8080",
            "local_id":"abroad","peer_id":"iran","tunnel_id":"demo",
            "peer_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "ca":"ca.pem","crl":"ca.crl","cert":"server.pem","key":"server-key.pem"
        }"#;
        let parsed: RuntimeConfig = serde_json::from_str(valid).unwrap();
        match parsed {
            RuntimeConfig::Serve { max_streams, .. } => assert_eq!(max_streams, 8),
            RuntimeConfig::Forward { .. } => panic!("expected serve config"),
        }
        let with_unknown = valid.replace(
            "\"mode\":\"serve\"",
            "\"mode\":\"serve\",\"unexpected\":true",
        );
        assert!(serde_json::from_str::<RuntimeConfig>(&with_unknown).is_err());
        let without_direction = valid.replace("\"direction\":\"direct\",", "");
        assert!(serde_json::from_str::<RuntimeConfig>(&without_direction).is_err());
        let without_crl = valid.replace("\"crl\":\"ca.crl\",", "");
        assert!(serde_json::from_str::<RuntimeConfig>(&without_crl).is_err());
    }

    #[test]
    fn support_report_redacts_paths_fingerprints_and_network_endpoints() {
        let config: RuntimeConfig = serde_json::from_str(
            r#"{
                "mode":"serve","direction":"direct","listen":"198.51.100.10:9443","target":"10.0.0.8:8080",
                "local_id":"abroad","peer_id":"iran","tunnel_id":"private-tunnel",
                "peer_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "ca":"/secret/ca.pem","crl":"/secret/ca.crl","cert":"/secret/node.pem","key":"/secret/private-key.pem"
            }"#,
        ).unwrap();
        let report = config.support_report().to_string();
        assert!(report.contains("private-tunnel"));
        assert!(!report.contains("198.51.100.10"));
        assert!(!report.contains("10.0.0.8"));
        assert!(!report.contains("aaaaaaaaaaaaaaaa"));
        assert!(!report.contains("/secret/"));
        assert!(!report.contains("private-key.pem"));
    }

    #[test]
    fn key_source_requires_exactly_one_configured_source() {
        assert!(private_key_path(None, None).is_err());
        assert!(private_key_path(Some(Path::new("node.key")), Some("node-key")).is_err());
        assert_eq!(
            key_source_description(Some(&PathBuf::from("hidden/path")), None),
            "filesystem (LAB compatibility)"
        );
        assert_eq!(
            key_source_description(None, Some("node-key")),
            "systemd credential (node-key)"
        );
    }

    #[test]
    fn config_accepts_systemd_credential_without_a_key_path() {
        let valid = r#"{
            "mode":"serve","direction":"direct","listen":"127.0.0.1:9443","target":"127.0.0.1:8080",
            "local_id":"abroad","peer_id":"iran","tunnel_id":"demo",
            "peer_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "ca":"ca.pem","crl":"ca.crl","cert":"server.pem","key_credential":"node-key"
        }"#;
        let parsed: RuntimeConfig = serde_json::from_str(valid).unwrap();
        assert!(parsed.plan().contains("systemd credential (node-key)"));
        let report = parsed.support_report();
        assert_eq!(
            report["configuration"]["private_key_source"],
            "systemd-credential"
        );
        assert_eq!(
            report["configuration"]["private_key_credential_id"],
            "node-key"
        );
        match parsed {
            RuntimeConfig::Serve {
                key,
                key_credential,
                ..
            } => {
                assert!(key.is_none());
                assert_eq!(key_credential.as_deref(), Some("node-key"));
            }
            RuntimeConfig::Forward { .. } => panic!("expected serve config"),
        }
    }

    #[test]
    fn target_must_be_specific_unicast() {
        assert!(validate_target("127.0.0.1:8080".parse().unwrap()).is_ok());
        assert!(validate_target("0.0.0.0:8080".parse().unwrap()).is_err());
        assert!(validate_target("224.0.0.1:8080".parse().unwrap()).is_err());
        assert!(validate_target("127.0.0.1:0".parse().unwrap()).is_err());
    }

    #[test]
    fn forward_listen_must_be_loopback_unless_explicitly_allowed() {
        let loopback: SocketAddr = "127.0.0.1:18080".parse().unwrap();
        let loopback6: SocketAddr = "[::1]:18080".parse().unwrap();
        let public: SocketAddr = "0.0.0.0:18080".parse().unwrap();
        assert!(validate_forward_listen(loopback, false).is_ok());
        assert!(validate_forward_listen(loopback6, false).is_ok());
        assert!(validate_forward_listen(public, false).is_err());
        assert!(validate_forward_listen(public, true).is_ok());
        assert!(validate_forward_listen("127.0.0.1:0".parse().unwrap(), true).is_err());
    }
}
