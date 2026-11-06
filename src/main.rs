// CLI entry point; configuration validation is side-effect-free until explicit apply.\nmod config;
mod handshake;
mod tls;
mod tunnel;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use handshake::{validate_fingerprint, validate_identity};
use std::path::PathBuf;
use tunnel::{ForwardOptions, ServerOptions};

#[derive(Debug, Parser)]
#[command(name = "baft-tunnel", version, about = "Authenticated BAFT-TUNNEL TCP forwarding")]
struct Cli { #[command(subcommand)] command: Command }

#[derive(Debug, Subcommand)]
enum Command {
    /// Accept authenticated peers and forward streams to one fixed destination.
    Serve {
        #[arg(long)] listen: String, #[arg(long)] target: String,
        #[arg(long)] local_id: String, #[arg(long)] peer_id: String, #[arg(long)] tunnel_id: String,
        #[arg(long)] peer_fingerprint: String, #[arg(long)] ca: PathBuf,
        #[arg(long)] cert: PathBuf, #[arg(long)] key: PathBuf,
        #[arg(long, default_value_t = 64)] max_streams: usize,
    },
    /// Listen locally and open authenticated multiplexed streams to a remote peer.
    Forward {
        #[arg(long)] listen: String, #[arg(long)] remote: String, #[arg(long)] server_name: String,
        #[arg(long)] local_id: String, #[arg(long)] peer_id: String, #[arg(long)] tunnel_id: String,
        #[arg(long)] peer_fingerprint: String, #[arg(long)] ca: PathBuf,
        #[arg(long)] cert: PathBuf, #[arg(long)] key: PathBuf,
        #[arg(long, default_value_t = 64)] max_streams: usize,
    },
    /// Validate or preview a JSON configuration without opening network listeners.
    Config { #[command(subcommand)] command: ConfigCommand },
    /// Start a validated JSON configuration; requires an explicit --confirm.
    Apply { #[arg(long)] config: PathBuf, #[arg(long)] confirm: bool },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Validate identities, sockets, certificates and keys.
    Validate { #[arg(long)] config: PathBuf },
    /// Show the effective configuration plan without applying it.
    Plan { #[arg(long)] config: PathBuf },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Serve { listen, target, local_id, peer_id, tunnel_id, peer_fingerprint, ca, cert, key, max_streams } => {
            validate_options(&local_id, &peer_id, &tunnel_id, &peer_fingerprint, max_streams)?;
            tunnel::serve(ServerOptions { listen, target, local_id, peer_id, tunnel_id, peer_fingerprint, ca, cert, key, max_streams }).await
        }
        Command::Forward { listen, remote, server_name, local_id, peer_id, tunnel_id, peer_fingerprint, ca, cert, key, max_streams } => {
            validate_options(&local_id, &peer_id, &tunnel_id, &peer_fingerprint, max_streams)?;
            tunnel::forward(ForwardOptions { listen, remote, server_name, local_id, peer_id, tunnel_id, peer_fingerprint, ca, cert, key, max_streams }).await
        }
        Command::Config { command: ConfigCommand::Validate { config: path } } => {
            config::load(&path)?.validate()?;
            println!("Configuration valid. No network listeners opened; no host settings changed.");
            Ok(())
        }
        Command::Config { command: ConfigCommand::Plan { config: path } } => {
            let config = config::load(&path)?;
            config.validate()?;
            println!("{}", config.plan());
            Ok(())
        }
        Command::Apply { config: path, confirm } => {
            if !confirm { bail!("apply requires explicit --confirm; run config plan first"); }
            let config = config::load(&path)?;
            config.validate()?;
            eprintln!("Applying validated tunnel configuration; no firewall or system settings will be changed.");
            config.apply().await
        }
    }
}

fn validate_options(local: &str, peer: &str, tunnel: &str, fingerprint: &str, streams: usize) -> Result<()> {
    validate_identity(local)?;
    validate_identity(peer)?;
    validate_identity(tunnel)?;
    validate_fingerprint(fingerprint)?;
    if local == peer { bail!("local and peer identities must differ"); }
    if !(1..=256).contains(&streams) { bail!("--max-streams must be between 1 and 256"); }
    Ok(())
}
