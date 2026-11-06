use std::{fs::File, io::BufReader, net::SocketAddr, path::{Path, PathBuf}};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::{
    handshake::{validate_fingerprint, validate_identity},
    tls,
    tunnel,
    tunnel::{ForwardOptions, ServerOptions},
};

fn default_max_streams() -> usize {
    64
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
        peer_fingerprint: String,
        ca: PathBuf,
        cert: PathBuf,
        key: PathBuf,
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
        peer_fingerprint: String,
        ca: PathBuf,
        cert: PathBuf,
        key: PathBuf,
        #[serde(default = "default_max_streams")]
        max_streams: usize,
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
                peer_fingerprint,
                ca,
                cert,
                key,
                max_streams,
            } => {
                validate_common(local_id, peer_id, tunnel_id, peer_fingerprint, *max_streams)?;
                parse_socket(listen, "listen")?;
                validate_target(parse_socket(target, "target")?)?;
                tls::server_config(ca, cert, key).context("server TLS material is invalid")?;
            }
            Self::Forward {
                listen,
                remote,
                server_name,
                local_id,
                peer_id,
                tunnel_id,
                peer_fingerprint,
                ca,
                cert,
                key,
                max_streams,
            } => {
                validate_common(local_id, peer_id, tunnel_id, peer_fingerprint, *max_streams)?;
                parse_socket(listen, "listen")?;
                validate_target(parse_socket(remote, "remote")?)?;
                tls::server_name(server_name)?;
                tls::client_config(ca, cert, key).context("client TLS material is invalid")?;
            }
        }
        Ok(())
    }

    pub fn plan(&self) -> String {
        match self {
            Self::Serve {
                listen,
                target,
                local_id,
                peer_id,
                tunnel_id,
                max_streams,
                ..
            } => format!(
                "Mode: serve\nListen: {listen}\nFixed destination: {target}\nLocal identity: {local_id}\nExpected peer: {peer_id}\nTunnel: {tunnel_id}\nMax streams: {max_streams}\nSecurity: TLS 1.3 + mutual certificate auth + SHA-256 peer pin\nHost changes: none"
            ),
            Self::Forward {
                listen,
                remote,
                server_name,
                local_id,
                peer_id,
                tunnel_id,
                max_streams,
                ..
            } => format!(
                "Mode: forward\nLocal listener: {listen}\nRemote peer: {remote}\nTLS server name: {server_name}\nLocal identity: {local_id}\nExpected peer: {peer_id}\nTunnel: {tunnel_id}\nMax streams: {max_streams}\nSecurity: TLS 1.3 + mutual certificate auth + SHA-256 peer pin\nHost changes: none"
            ),
        }
    }

    pub async fn apply(self) -> Result<()> {
        match self {
            Self::Serve {
                listen,
                target,
                local_id,
                peer_id,
                tunnel_id,
                peer_fingerprint,
                ca,
                cert,
                key,
                max_streams,
            } => {
                tunnel::serve(ServerOptions {
                    listen,
                    target,
                    local_id,
                    peer_id,
                    tunnel_id,
                    peer_fingerprint,
                    ca,
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
                peer_fingerprint,
                ca,
                cert,
                key,
                max_streams,
            } => {
                tunnel::forward(ForwardOptions {
                    listen,
                    remote,
                    server_name,
                    local_id,
                    peer_id,
                    tunnel_id,
                    peer_fingerprint,
                    ca,
                    cert,
                    key,
                    max_streams,
                })
                .await
            }
        }
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
    if !(1..=256).contains(&streams) {
        bail!("max_streams must be between 1 and 256");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_rejects_unknown_fields_and_defaults_stream_limit() {
        let valid = r#"{
            "mode":"serve","listen":"127.0.0.1:9443","target":"127.0.0.1:8080",
            "local_id":"abroad","peer_id":"iran","tunnel_id":"demo",
            "peer_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "ca":"ca.pem","cert":"server.pem","key":"server-key.pem"
        }"#;
        let parsed: RuntimeConfig = serde_json::from_str(valid).unwrap();
        match parsed {
            RuntimeConfig::Serve { max_streams, .. } => assert_eq!(max_streams, 64),
            RuntimeConfig::Forward { .. } => panic!("expected serve config"),
        }
        let with_unknown = valid.replace(
            "\"mode\":\"serve\"",
            "\"mode\":\"serve\",\"unexpected\":true",
        );
        assert!(serde_json::from_str::<RuntimeConfig>(&with_unknown).is_err());
    }

    #[test]
    fn target_must_be_specific_unicast() {
        assert!(validate_target("127.0.0.1:8080".parse().unwrap()).is_ok());
        assert!(validate_target("0.0.0.0:8080".parse().unwrap()).is_err());
        assert!(validate_target("224.0.0.1:8080".parse().unwrap()).is_err());
        assert!(validate_target("127.0.0.1:0".parse().unwrap()).is_err());
    }
}
