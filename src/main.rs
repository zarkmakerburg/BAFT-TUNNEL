mod authorization;
mod config;
mod handshake;
#[allow(dead_code)]
mod noise;
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
    /// Print a redacted diagnostic bundle; never includes key material or network endpoints.
    SupportBundle {
        #[arg(long)]
        config: PathBuf,
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
                key,
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
                key,
                max_streams,
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
