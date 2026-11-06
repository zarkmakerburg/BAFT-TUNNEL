mod handshake;
mod tls;
mod tunnel;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use handshake::{validate_fingerprint, validate_identity};
use std::path::PathBuf;
use tunnel::{ForwardOptions, ServerOptions};

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
        #[arg(long)]
        peer_fingerprint: String,
        #[arg(long)]
        ca: PathBuf,
        #[arg(long)]
        cert: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value_t = 64)]
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
        #[arg(long)]
        peer_fingerprint: String,
        #[arg(long)]
        ca: PathBuf,
        #[arg(long)]
        cert: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value_t = 64)]
        max_streams: usize,
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
            peer_fingerprint,
            ca,
            cert,
            key,
            max_streams,
        } => {
            validate_options(
                &local_id,
                &peer_id,
                &tunnel_id,
                &peer_fingerprint,
                max_streams,
            )?;
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
        Command::Forward {
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
            validate_options(
                &local_id,
                &peer_id,
                &tunnel_id,
                &peer_fingerprint,
                max_streams,
            )?;
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

fn validate_options(
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
        bail!("--max-streams must be between 1 and 256");
    }
    Ok(())
}
