mod authorization;
mod config;
mod engine;
mod handshake;
pub mod key_custody;
mod noise;
mod setup;
mod tls;
mod tunnel;

use anyhow::{Result, bail};
use authorization::ConnectionDirection;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "baft-tunnel",
    version,
    about = "Authenticated BAFT-TUNNEL TCP forwarding"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Accept authenticated peers and forward streams to one fixed destination.
    Serve {
        #[arg(long)]
        listen: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        local_id: String,
        #[arg(long)]
        peer_id: String,
        #[arg(long)]
        tunnel_id: String,
        #[arg(long, value_enum)]
        direction: ConnectionDirection,
        #[arg(long)]
        peer_fingerprint: String,
        #[arg(long)]
        ca: PathBuf,
        #[arg(long)]
        crl: PathBuf,
        #[arg(long)]
        cert: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value_t = 8)]
        max_streams: usize,
    },
    /// Listen locally and open authenticated multiplexed streams to a remote peer.
    Forward {
        #[arg(long)]
        listen: String,
        #[arg(long)]
        remote: String,
        #[arg(long)]
        server_name: String,
        #[arg(long)]
        local_id: String,
        #[arg(long)]
        peer_id: String,
        #[arg(long)]
        tunnel_id: String,
        #[arg(long, value_enum)]
        direction: ConnectionDirection,
        #[arg(long)]
        peer_fingerprint: String,
        #[arg(long)]
        ca: PathBuf,
        #[arg(long)]
        crl: PathBuf,
        #[arg(long)]
        cert: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value_t = 8)]
        max_streams: usize,
        /// Permit a non-loopback --listen address (unauthenticated local clients can then use the tunnel).
        #[arg(long)]
        allow_non_loopback_listen: bool,
    },
    /// Validate or preview a JSON configuration without opening network listeners.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Start a validated JSON configuration; requires an explicit --confirm.
    Apply {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        confirm: bool,
    },
    /// Guided setup (slice S1: plan only; writes nothing and opens no sockets).
    Setup {
        #[command(subcommand)]
        command: SetupCommand,
    },
    /// Create a Noise static keypair: the private key is written to --out (mode 0600, never
    /// overwritten); the public key is printed for enrolment in the peer's config.
    NoiseKeygen {
        #[arg(long)]
        out: PathBuf,
    },
    /// Print a redacted diagnostic bundle; never includes key material or network endpoints.
    SupportBundle {
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum SetupCommand {
    /// Show the configuration and files setup would create. Changes nothing.
    Plan {
        #[arg(long, value_enum)]
        engine: config::EngineKind,
        #[arg(long, value_enum)]
        role: setup::Role,
        #[arg(long, value_enum)]
        direction: ConnectionDirection,
        #[arg(long)]
        node_id: String,
        #[arg(long)]
        peer_id: String,
        #[arg(long)]
        tunnel_id: String,
        /// The one port you choose: the service exposed through the tunnel.
        #[arg(long)]
        service_port: u16,
        #[arg(long, default_value_t = setup::DEFAULT_TUNNEL_PORT)]
        tunnel_port: u16,
        #[arg(long)]
        peer_host: Option<String>,
        #[arg(long)]
        server_name: Option<String>,
        #[arg(long, default_value = "/etc/baft-tunnel")]
        dir: PathBuf,
        /// Print only the configuration template as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Create this node's key and a public pairing bundle (noise-tcp). Never overwrites.
    Init {
        #[arg(long, value_enum)]
        engine: config::EngineKind,
        #[arg(long, value_enum)]
        role: setup::Role,
        #[arg(long, value_enum)]
        direction: ConnectionDirection,
        #[arg(long)]
        node_id: String,
        #[arg(long)]
        peer_id: String,
        #[arg(long)]
        tunnel_id: String,
        #[arg(long)]
        service_port: u16,
        #[arg(long, default_value_t = setup::DEFAULT_TUNNEL_PORT)]
        tunnel_port: u16,
        #[arg(long)]
        peer_host: Option<String>,
        #[arg(long, default_value = "/etc/baft-tunnel")]
        dir: PathBuf,
        /// Where to write the public bundle (default: DIR/bundle.json).
        #[arg(long)]
        bundle_out: Option<PathBuf>,
    },
    /// Check the peer's bundle (and the checksum you compared out of band) and write config.json.
    Join {
        #[arg(long, default_value = "/etc/baft-tunnel")]
        dir: PathBuf,
        #[arg(long)]
        bundle: PathBuf,
        /// Checksum printed by `setup init` on the peer node.
        #[arg(long)]
        checksum: String,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Validate identities, sockets, certificates and keys.
    Validate {
        #[arg(long)]
        config: PathBuf,
    },
    /// Show the effective configuration plan without applying it.
    Plan {
        #[arg(long)]
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Serve {
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
        } => {
            let config = config::RuntimeConfig::Serve {
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
                key: Some(key),
                key_credential: None,
                engine: config::EngineKind::TcpTls,
                noise_peer_public: None,
                max_streams,
            };
            config.validate()?;
            config.apply().await
        }
        Command::Forward {
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
            allow_non_loopback_listen,
        } => {
            let config = config::RuntimeConfig::Forward {
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
                key: Some(key),
                key_credential: None,
                engine: config::EngineKind::TcpTls,
                noise_peer_public: None,
                max_streams,
                allow_non_loopback_listen,
            };
            config.validate()?;
            config.apply().await
        }
        Command::Config {
            command: ConfigCommand::Validate { config: path },
        } => {
            config::load(&path)?.validate()?;
            println!("Configuration valid. No network listeners opened; no host settings changed.");
            Ok(())
        }
        Command::Config {
            command: ConfigCommand::Plan { config: path },
        } => {
            let config = config::load(&path)?;
            config.validate()?;
            println!("{}", config.plan());
            Ok(())
        }
        Command::Apply {
            config: path,
            confirm,
        } => {
            if !confirm {
                bail!("apply requires explicit --confirm; run config plan first");
            }
            let config = config::load(&path)?;
            config.validate()?;
            eprintln!(
                "Applying validated tunnel configuration; no firewall or system settings will be changed."
            );
            config.apply().await
        }
        Command::Setup {
            command:
                SetupCommand::Plan {
                    engine,
                    role,
                    direction,
                    node_id,
                    peer_id,
                    tunnel_id,
                    service_port,
                    tunnel_port,
                    peer_host,
                    server_name,
                    dir,
                    json,
                },
        } => {
            let plan = setup::plan(&setup::PlanRequest {
                engine,
                role,
                direction,
                node_id,
                peer_id,
                tunnel_id,
                service_port,
                tunnel_port,
                peer_host,
                server_name,
                dir,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&plan.config)?);
            } else {
                println!("Setup plan (nothing is written or started):");
                println!("{}", serde_json::to_string_pretty(&plan.config)?);
                println!("Files that setup will create or expect:");
                for file in &plan.files {
                    println!("  {} [{}] {}", file.path.display(), file.mode, file.note);
                }
                println!("Still needed before the tunnel can start:");
                for item in &plan.pending {
                    println!("  - {item}");
                }
            }
            Ok(())
        }
        Command::Setup {
            command:
                SetupCommand::Init {
                    engine,
                    role,
                    direction,
                    node_id,
                    peer_id,
                    tunnel_id,
                    service_port,
                    tunnel_port,
                    peer_host,
                    dir,
                    bundle_out,
                },
        } => {
            let result = setup::init(
                &setup::PlanRequest {
                    engine,
                    role,
                    direction,
                    node_id,
                    peer_id,
                    tunnel_id,
                    service_port,
                    tunnel_port,
                    peer_host,
                    server_name: None,
                    dir,
                },
                bundle_out.as_deref(),
            )?;
            println!("Key created (mode 0600, never exported).");
            println!("Public bundle: {}", result.bundle_path.display());
            println!(
                "Checksum (compare with the other node out of band): {}",
                result.checksum
            );
            Ok(())
        }
        Command::Setup {
            command:
                SetupCommand::Join {
                    dir,
                    bundle,
                    checksum,
                },
        } => {
            let path = setup::join(&dir, &bundle, &checksum)?;
            println!("Configuration written: {}", path.display());
            println!(
                "Review it with: baft-tunnel config plan --config {}",
                path.display()
            );
            Ok(())
        }
        Command::NoiseKeygen { out } => {
            let public = engine::write_noise_keypair(&out)?;
            println!("{public}");
            Ok(())
        }
        Command::SupportBundle { config: path } => {
            let config = config::load(&path)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&config.support_report())?
            );
            Ok(())
        }
    }
}
