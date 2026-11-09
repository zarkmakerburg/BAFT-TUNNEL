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
    engine::NoiseSettings,
    handshake::{validate_fingerprint, validate_identity},
    tls, tunnel,
    tunnel::{ForwardOptions, ServerOptions},
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[value(rename_all = "kebab-case")]
pub enum EngineKind {
    #[default]
    TcpTls,
    NoiseTcp,
    /// BAFT main engine: a separate, pinned Go process configured by `baft_engine` (ADR-0019).
    Baft,
}

/// Where the pinned BAFT engine binary lives and the SHA-256 it must have (ADR-0019).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaftSettings {
    #[serde(default = "default_baft_binary")]
    pub binary: PathBuf,
    pub sha256: String,
    /// Engine store (see `baft-tunnel engine install`). When set, `binary` is ignored: the
    /// launcher runs the store's verified `current` release, which must have this `sha256`.
    #[serde(default)]
    pub store: Option<PathBuf>,
}

fn default_baft_binary() -> PathBuf {
    PathBuf::from("/usr/local/lib/baft-tunnel/baft")
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TcpTls => "tcp-tls",
            Self::NoiseTcp => "noise-tcp",
            Self::Baft => "baft",
        }
    }
}

/// Warn a week ahead: CRLs are renewed weekly (`baft-pki.sh renew-crl`, 30-day validity).
pub(crate) const CRL_WARNING_SECONDS: i64 = 7 * 24 * 3600;

pub(crate) fn default_max_streams() -> usize {
    8
}

#[derive(Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RuntimeConfig {
    Serve {
        listen: String,
        /// Direct: the fixed destination dialled per stream. Empty for Reverse.
        #[serde(default)]
        target: String,
        /// Reverse only (ADR-0021): the user-facing ingress listener on this (public) node.
        #[serde(default)]
        ingress: String,
        /// Reverse only: allow `ingress` on a non-loopback address (explicit, unauthenticated).
        #[serde(default)]
        allow_non_loopback_ingress: bool,
        local_id: String,
        peer_id: String,
        tunnel_id: String,
        direction: ConnectionDirection,
        #[serde(default)]
        peer_fingerprint: String,
        #[serde(default)]
        ca: PathBuf,
        #[serde(default)]
        crl: PathBuf,
        #[serde(default)]
        cert: PathBuf,
        key: Option<PathBuf>,
        #[serde(default)]
        key_credential: Option<String>,
        #[serde(default)]
        engine: EngineKind,
        #[serde(default)]
        noise_peer_public: Option<String>,
        #[serde(default = "default_max_streams")]
        max_streams: usize,
        #[serde(default)]
        baft: Option<BaftSettings>,
    },
    Forward {
        /// Direct: local ingress listener. Empty for Reverse (the peer owns the ingress).
        #[serde(default)]
        listen: String,
        /// Reverse only (ADR-0021): the local service this (service) node relays streams to.
        #[serde(default)]
        target: String,
        remote: String,
        #[serde(default)]
        server_name: String,
        local_id: String,
        peer_id: String,
        tunnel_id: String,
        direction: ConnectionDirection,
        #[serde(default)]
        peer_fingerprint: String,
        #[serde(default)]
        ca: PathBuf,
        #[serde(default)]
        crl: PathBuf,
        #[serde(default)]
        cert: PathBuf,
        key: Option<PathBuf>,
        #[serde(default)]
        key_credential: Option<String>,
        #[serde(default)]
        engine: EngineKind,
        #[serde(default)]
        noise_peer_public: Option<String>,
        #[serde(default = "default_max_streams")]
        max_streams: usize,
        /// Allow the local forwarder to listen on a non-loopback address. Anyone who can reach
        /// that address can use the tunnel without authentication, so it must be explicit.
        #[serde(default)]
        allow_non_loopback_listen: bool,
        #[serde(default)]
        baft: Option<BaftSettings>,
    },
}

impl RuntimeConfig {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Serve {
                listen,
                target,
                ingress,
                allow_non_loopback_ingress,
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
                engine,
                noise_peer_public,
                max_streams,
                baft,
            } => {
                validate_common(local_id, peer_id, tunnel_id, *max_streams)?;
                parse_socket(listen, "listen")?;
                // The baft engine keeps its previous behaviour (direction is a label there; it
                // is not part of the Rust Reverse mode, ADR-0021 Amendment 1).
                let reverse =
                    *direction == ConnectionDirection::Reverse && *engine != EngineKind::Baft;
                match reverse {
                    false => {
                        if !ingress.is_empty() {
                            bail!("ingress is only valid for reverse");
                        }
                        validate_target(parse_socket(target, "target")?)?;
                    }
                    true => {
                        if !target.is_empty() {
                            bail!("a reverse public node has no target; the service node owns it");
                        }
                        validate_forward_listen(
                            parse_socket(ingress, "ingress")?,
                            *allow_non_loopback_ingress,
                        )?;
                    }
                }
                let key_path = private_key_path(key.as_deref(), key_credential.as_deref())?;
                match engine {
                    EngineKind::TcpTls => {
                        validate_fingerprint(peer_fingerprint)?;
                        require_tls_paths(&[ca, crl, cert])?;
                        reject_noise_fields(noise_peer_public.as_deref())?;
                        tls::server_config(ca, crl, cert, &key_path)
                            .context("server TLS material is invalid")?;
                    }
                    EngineKind::NoiseTcp => {
                        validate_noise(
                            &[ca, crl, cert],
                            peer_fingerprint,
                            &key_path,
                            noise_peer_public.as_deref(),
                        )?;
                    }
                    EngineKind::Baft => {
                        validate_baft(
                            &[ca, crl, cert],
                            peer_fingerprint,
                            false,
                            noise_peer_public.as_deref(),
                            baft.as_ref(),
                        )?;
                        tls::server_config(ca, crl, cert, &key_path)
                            .context("server TLS material is invalid")?;
                    }
                }
            }
            Self::Forward {
                listen,
                target,
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
                engine,
                noise_peer_public,
                max_streams,
                allow_non_loopback_listen,
                baft,
            } => {
                validate_common(local_id, peer_id, tunnel_id, *max_streams)?;
                let reverse =
                    *direction == ConnectionDirection::Reverse && *engine != EngineKind::Baft;
                match reverse {
                    false => {
                        if !target.is_empty() {
                            bail!("target is only valid for reverse");
                        }
                        validate_forward_listen(
                            parse_socket(listen, "listen")?,
                            *allow_non_loopback_listen,
                        )?;
                    }
                    true => {
                        if !listen.is_empty() {
                            bail!(
                                "a reverse service node has no listener; the peer owns the ingress"
                            );
                        }
                        validate_target(parse_socket(target, "target")?)?;
                    }
                }
                validate_target(parse_socket(remote, "remote")?)?;
                let key_path = private_key_path(key.as_deref(), key_credential.as_deref())?;
                match engine {
                    EngineKind::TcpTls => {
                        validate_fingerprint(peer_fingerprint)?;
                        require_tls_paths(&[ca, crl, cert])?;
                        reject_noise_fields(noise_peer_public.as_deref())?;
                        tls::server_name(server_name)?;
                        tls::client_config(ca, crl, cert, &key_path)
                            .context("client TLS material is invalid")?;
                    }
                    EngineKind::NoiseTcp => {
                        if !server_name.is_empty() {
                            bail!("server_name is not used by the noise-tcp engine");
                        }
                        validate_noise(
                            &[ca, crl, cert],
                            peer_fingerprint,
                            &key_path,
                            noise_peer_public.as_deref(),
                        )?;
                    }
                    EngineKind::Baft => {
                        validate_baft(
                            &[ca, crl, cert],
                            peer_fingerprint,
                            true,
                            noise_peer_public.as_deref(),
                            baft.as_ref(),
                        )?;
                        tls::server_name(server_name)?;
                        tls::client_config(ca, crl, cert, &key_path)
                            .context("client TLS material is invalid")?;
                    }
                }
            }
        }
        if self.engine() != EngineKind::NoiseTcp {
            self.warn_if_crl_expiring();
        }
        Ok(())
    }

    fn engine(&self) -> EngineKind {
        match self {
            Self::Serve { engine, .. } | Self::Forward { engine, .. } => *engine,
        }
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
                "WARNING: certificate revocation list has expired; TLS connections will fail closed until it is refreshed (baft-pki.sh renew-crl, then push-crl.sh)"
            ),
            Ok(remaining) if remaining < CRL_WARNING_SECONDS => eprintln!(
                "WARNING: certificate revocation list expires in {} hours; renew it with baft-pki.sh renew-crl and distribute it with push-crl.sh",
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
                engine,
                ..
            } => format!(
                "Mode: serve\nDirection: {}\nListen: {listen}\nFixed destination: {target}\nLocal identity: {local_id}\nExpected peer: {peer_id}\nTunnel: {tunnel_id}\nMax streams: {max_streams}\nPrivate key source: {}\nSecurity: {}\nHost changes: none",
                direction.as_str(),
                key_source_description(key.as_ref(), key_credential.as_deref()),
                security_description(*engine)
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
                engine,
                ..
            } => format!(
                "Mode: forward\nDirection: {}\nLocal listener: {listen}\nRemote peer: {remote}\nTLS server name: {server_name}\nLocal identity: {local_id}\nExpected peer: {peer_id}\nTunnel: {tunnel_id}\nMax streams: {max_streams}\nPrivate key source: {}\nSecurity: {}\nHost changes: none",
                direction.as_str(),
                key_source_description(key.as_ref(), key_credential.as_deref()),
                security_description(*engine)
            ),
        }
    }

    pub fn engine_name(&self) -> &'static str {
        let (Self::Serve { engine, .. } | Self::Forward { engine, .. }) = self;
        engine.as_str()
    }

    /// `(crl, cert)` paths when the engine is TLS (they are empty for noise-tcp).
    pub fn tls_files(&self) -> Option<(&Path, &Path)> {
        match self {
            Self::Serve {
                engine: EngineKind::TcpTls,
                crl,
                cert,
                ..
            }
            | Self::Forward {
                engine: EngineKind::TcpTls,
                crl,
                cert,
                ..
            } => Some((crl, cert)),
            _ => None,
        }
    }

    /// `(local_id, peer_id, tunnel_id)`: node identities, not secrets.
    pub fn ids(&self) -> (&str, &str, &str) {
        match self {
            Self::Serve {
                local_id,
                peer_id,
                tunnel_id,
                ..
            }
            | Self::Forward {
                local_id,
                peer_id,
                tunnel_id,
                ..
            } => (local_id, peer_id, tunnel_id),
        }
    }

    /// The peer's tunnel socket when this node dials (`forward`); `None` when the peer dials in.
    pub fn remote(&self) -> Option<&str> {
        match self {
            Self::Serve { .. } => None,
            Self::Forward { remote, .. } => Some(remote),
        }
    }

    pub fn listen_addr(&self) -> &str {
        match self {
            Self::Serve { listen, .. } | Self::Forward { listen, .. } => listen,
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
        if self.engine() == EngineKind::Baft {
            return crate::baft_engine::run(self).await;
        }
        match self {
            Self::Serve {
                listen,
                target,
                ingress,
                allow_non_loopback_ingress: _,
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
                engine,
                noise_peer_public,
                max_streams,
                baft: _,
            } => {
                let key = private_key_path(key.as_deref(), key_credential.as_deref())?;
                let noise = noise_settings(engine, &key, noise_peer_public);
                tunnel::serve(ServerOptions {
                    ingress: (!ingress.is_empty()).then_some(ingress),
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
                    noise,
                    max_streams,
                })
                .await
            }
            Self::Forward {
                listen,
                target,
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
                engine,
                noise_peer_public,
                max_streams,
                allow_non_loopback_listen: _,
                baft: _,
            } => {
                let key = private_key_path(key.as_deref(), key_credential.as_deref())?;
                let noise = noise_settings(engine, &key, noise_peer_public);
                tunnel::forward(ForwardOptions {
                    target: (!target.is_empty()).then_some(target),
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
                    noise,
                    max_streams,
                })
                .await
            }
        }
    }
}

fn noise_settings(
    engine: EngineKind,
    key: &Path,
    peer_public: Option<String>,
) -> Option<NoiseSettings> {
    match (engine, peer_public) {
        (EngineKind::NoiseTcp, Some(peer_public)) => Some(NoiseSettings {
            private_key: key.to_path_buf(),
            peer_public,
        }),
        _ => None,
    }
}

pub(crate) fn private_key_path(key: Option<&Path>, credential_id: Option<&str>) -> Result<PathBuf> {
    match (key, credential_id) {
        (Some(path), None) => Ok(path.to_path_buf()),
        (None, Some(identifier)) => crate::key_custody::systemd_credential_path(identifier),
        (Some(_), Some(_)) => bail!("configure exactly one of key or key_credential"),
        (None, None) => bail!("configure exactly one of key or key_credential"),
    }
}

fn validate_baft(
    paths: &[&PathBuf],
    peer_fingerprint: &str,
    is_dialer: bool,
    noise_peer_public: Option<&str>,
    baft: Option<&BaftSettings>,
) -> Result<()> {
    require_tls_paths(paths)?;
    reject_noise_fields(noise_peer_public)?;
    if !peer_fingerprint.is_empty() {
        if !is_dialer {
            bail!(
                "peer_fingerprint is not used by a baft listener (it admits clients by CA, identity allowlist and revocation)"
            );
        }
        if peer_fingerprint.contains(',') {
            bail!(
                "the baft engine pins exactly one listener certificate; a rollover list is not supported"
            );
        }
        validate_fingerprint(peer_fingerprint)?;
    }
    let settings = baft.context("engine baft requires a baft {binary, sha256} section")?;
    if !settings.binary.is_absolute() {
        bail!("baft.binary must be an absolute path");
    }
    if settings.store.as_ref().is_some_and(|s| !s.is_absolute()) {
        bail!("baft.store must be an absolute path");
    }
    if settings.sha256.len() != 64 || !settings.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("baft.sha256 must be a 64-digit hexadecimal SHA-256");
    }
    Ok(())
}

fn security_description(engine: EngineKind) -> &'static str {
    match engine {
        EngineKind::TcpTls => "TLS 1.3 + mutual certificate auth + SHA-256 peer pin",
        EngineKind::NoiseTcp => {
            "Noise IK (X25519/ChaChaPoly/BLAKE2s) + pinned peer static key, no certificates"
        }
        EngineKind::Baft => {
            "BAFT/1 over HTTP/2 + TLS 1.3 mutual auth + URI identity allowlist + revocation on both sides + optional single listener certificate pin, separate Go process"
        }
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

fn require_tls_paths(paths: &[&PathBuf]) -> Result<()> {
    if paths.iter().any(|path| path.as_os_str().is_empty()) {
        bail!("engine tcp-tls requires ca, crl and cert");
    }
    Ok(())
}

fn reject_noise_fields(noise_peer_public: Option<&str>) -> Result<()> {
    if noise_peer_public.is_some() {
        bail!("noise_peer_public is only valid with engine noise-tcp");
    }
    Ok(())
}

/// Fail closed on ambiguous configuration: a Noise tunnel must not also carry TLS material.
fn validate_noise(
    tls_paths: &[&PathBuf],
    peer_fingerprint: &str,
    key_path: &Path,
    noise_peer_public: Option<&str>,
) -> Result<()> {
    if tls_paths.iter().any(|path| !path.as_os_str().is_empty()) || !peer_fingerprint.is_empty() {
        bail!("ca, crl, cert and peer_fingerprint must be omitted with engine noise-tcp");
    }
    let peer = noise_peer_public.context("engine noise-tcp requires noise_peer_public")?;
    crate::engine::NoiseSettings {
        private_key: key_path.to_path_buf(),
        peer_public: peer.to_owned(),
    }
    .validate()
}

fn validate_common(local: &str, peer: &str, tunnel: &str, streams: usize) -> Result<()> {
    validate_identity(local)?;
    validate_identity(peer)?;
    validate_identity(tunnel)?;
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
        assert!(validate_common("node-a", "node-b", "tunnel", 1).is_ok());
        assert!(validate_common("node-a", "node-b", "tunnel", 8).is_ok());
        assert!(validate_common("node-a", "node-b", "tunnel", 9).is_err());
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
        // A missing CRL must still fail closed for the TLS engine (at validation).
        let without_crl = valid.replace("\"crl\":\"ca.crl\",", "");
        let parsed: RuntimeConfig = serde_json::from_str(&without_crl).unwrap();
        assert!(parsed.validate().is_err());
    }

    #[test]
    fn reverse_config_assigns_ingress_and_target_to_the_right_node() {
        let dir = std::env::temp_dir().join(format!("baft-rev-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("k.noise");
        let _ = std::fs::remove_file(&key);
        let public = crate::engine::write_noise_keypair(&key).unwrap();
        let cfg = |mode: &str, fields: &str| {
            let text = format!(
                r#"{{"mode":"{mode}","engine":"noise-tcp","direction":"reverse",{fields},"local_id":"abroad","peer_id":"iran","tunnel_id":"demo","key":"{}","noise_peer_public":"{public}"}}"#,
                key.display()
            );
            serde_json::from_str::<RuntimeConfig>(&text)
                .unwrap()
                .validate()
        };
        let remote = r#""remote":"203.0.113.7:9443""#;
        // Public node: tunnel listener plus ingress, no target.
        cfg(
            "serve",
            r#""listen":"0.0.0.0:9443","ingress":"127.0.0.1:8080""#,
        )
        .unwrap();
        assert!(cfg("serve", r#""listen":"0.0.0.0:9443""#).is_err());
        assert!(
            cfg(
                "serve",
                r#""listen":"0.0.0.0:9443","ingress":"127.0.0.1:8080","target":"127.0.0.1:1""#
            )
            .is_err()
        );
        assert!(
            cfg(
                "serve",
                r#""listen":"0.0.0.0:9443","ingress":"0.0.0.0:8080""#
            )
            .is_err()
        );
        // Service node: dials out and relays to the local service, no listener.
        cfg("forward", &format!(r#""target":"127.0.0.1:8080",{remote}"#)).unwrap();
        assert!(cfg("forward", remote).is_err());
        assert!(
            cfg(
                "forward",
                &format!(r#""listen":"127.0.0.1:8080","target":"127.0.0.1:8080",{remote}"#)
            )
            .is_err()
        );
        // Direct keeps rejecting the reverse-only fields.
        let direct = format!(
            r#"{{"mode":"serve","engine":"noise-tcp","direction":"direct","listen":"0.0.0.0:9443","target":"127.0.0.1:8080","ingress":"127.0.0.1:9","local_id":"abroad","peer_id":"iran","tunnel_id":"demo","key":"{}","noise_peer_public":"{public}"}}"#,
            key.display()
        );
        assert!(
            serde_json::from_str::<RuntimeConfig>(&direct)
                .unwrap()
                .validate()
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn noise_engine_config_is_strict_about_ambiguous_material() {
        let dir = std::env::temp_dir().join(format!("baft-noise-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("k.noise");
        let _ = std::fs::remove_file(&key);
        let public = crate::engine::write_noise_keypair(&key).unwrap();
        let base = |extra: &str| {
            format!(
                r#"{{"mode":"serve","engine":"noise-tcp","direction":"direct","listen":"127.0.0.1:9443","target":"127.0.0.1:8080","local_id":"abroad","peer_id":"iran","tunnel_id":"demo","key":"{}"{extra}}}"#,
                key.display()
            )
        };
        let ok = base(&format!(r#","noise_peer_public":"{public}""#));
        serde_json::from_str::<RuntimeConfig>(&ok)
            .unwrap()
            .validate()
            .unwrap();
        // Missing peer key, TLS material mixed in, bad key length: all rejected.
        assert!(
            serde_json::from_str::<RuntimeConfig>(&base(""))
                .unwrap()
                .validate()
                .is_err()
        );
        let mixed = base(&format!(r#","noise_peer_public":"{public}","ca":"ca.pem""#));
        assert!(
            serde_json::from_str::<RuntimeConfig>(&mixed)
                .unwrap()
                .validate()
                .is_err()
        );
        let short = base(r#","noise_peer_public":"abcd""#);
        assert!(
            serde_json::from_str::<RuntimeConfig>(&short)
                .unwrap()
                .validate()
                .is_err()
        );
        // The TLS engine must not accept a stray Noise key.
        let stray = ok.replace("noise-tcp", "tcp-tls");
        assert!(
            serde_json::from_str::<RuntimeConfig>(&stray)
                .unwrap()
                .validate()
                .is_err()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn config_parse_and_validate_never_panic_on_mutated_input() {
        let valid = r#"{
            "mode":"serve","direction":"direct","listen":"127.0.0.1:9443","target":"127.0.0.1:8080",
            "local_id":"abroad","peer_id":"iran","tunnel_id":"demo",
            "peer_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "ca":"ca.pem","crl":"ca.crl","cert":"server.pem","key":"server-key.pem"
        }"#;
        let mut state = 0x1234_5678_9abc_def1_u64;
        for _case in 0..2048 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let mut bytes = valid.as_bytes().to_vec();
            for i in 0..(1 + (state as usize) % 4) {
                let pos = ((state >> (7 * i)) as usize) % bytes.len();
                match (state >> 5) % 3 {
                    0 => bytes[pos] = (state >> (2 * i)) as u8,
                    1 => {
                        bytes.remove(pos);
                    }
                    _ => bytes.insert(pos, (state >> i) as u8),
                }
            }
            if let Ok(text) = std::str::from_utf8(&bytes)
                && let Ok(config) = serde_json::from_str::<RuntimeConfig>(text)
            {
                let _ = config.validate();
            }
        }
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
