mod agent;
mod apply;
mod authorization;
#[allow(dead_code)] // the renderer API is used by tests and later phases (ADR-0019)
mod baft_engine;
mod capabilities;
mod config;
mod engine;
mod engine_store;
mod handshake;
pub mod key_custody;
mod noise;
mod observe;
#[allow(dead_code)] // manifest API is consumed by packaging and the registry (ADR-0020)
mod runtime_host;
mod session_status;
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
    /// Install, verify, upgrade and roll back pinned engine binaries (nothing is downloaded).
    Engine {
        #[command(subcommand)]
        command: EngineCommand,
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
    /// Create a random API token file for the local agent (mode 0600, never overwritten).
    AgentToken {
        #[arg(long)]
        out: PathBuf,
    },
    /// Interactive menu (Persian/English) over the install, setup and service commands.
    /// Runs scripts/menu.sh installed next to the binary (PREFIX/lib/baft-tunnel).
    Menu {
        /// Passed to the menu, for example `--lang fa` or `--plain`.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run the local API (loopback only, bearer token): status, and validated desired state storage. Never starts a tunnel; its only writes are the state directory and the confirmed local setup stages of `POST /v1/setup/apply`.
    Agent {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        token_file: PathBuf,
        #[arg(long, default_value = "127.0.0.1:18080")]
        listen: String,
        /// Directory (must exist) for desired.json and audit.log; enables PUT/GET /v1/desired.
        #[arg(long)]
        state_dir: Option<PathBuf>,
        /// systemd instance (`baft-tunnel@NAME`) to report on: unit state, authorized session,
        /// traffic counters and last failure stage from /run/baft-tunnel/NAME.
        #[arg(long)]
        instance: Option<String>,
        /// Run although the effective user is root. Not recommended: the agent is designed for
        /// the unprivileged `baft-tunnel` user (see docs/SECURITY_ASSESSMENT.md, SA-01).
        #[arg(long)]
        allow_root: bool,
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
        /// systemd instance name: write NAME.json (read by baft-tunnel@NAME) instead of config.json.
        #[arg(long)]
        instance: Option<String>,
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
        /// systemd instance name: write NAME.json (read by baft-tunnel@NAME) instead of config.json.
        #[arg(long)]
        instance: Option<String>,
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
enum EngineCommand {
    /// Check a binary against the manifest's pinned SHA-256 (touches nothing).
    Check {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        binary: PathBuf,
    },
    /// Verify and install a binary; the previous current release is kept for rollback.
    Install {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        binary: PathBuf,
    },
    /// Re-verify the installed current release.
    Verify {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        id: String,
    },
    /// Switch back to the previous release (verified first).
    Rollback {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        id: String,
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

/// Print errors as one readable line (with their causes) and never a backtrace, even when
/// RUST_BACKTRACE is set: operators see `Error: ...`, not Rust internals.
#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// `PREFIX/lib/baft-tunnel/scripts/menu.sh` for a binary in `PREFIX/bin`, else the default prefix.
fn menu_script() -> PathBuf {
    let installed = std::env::current_exe().ok().and_then(|exe| {
        let prefix = exe.parent()?.parent()?.to_path_buf();
        Some(prefix.join("lib/baft-tunnel/scripts/menu.sh"))
    });
    match installed {
        Some(path) if path.is_file() => path,
        _ => PathBuf::from("/usr/local/lib/baft-tunnel/scripts/menu.sh"),
    }
}

async fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Menu { args } => {
            use std::os::unix::process::CommandExt;
            let script = menu_script();
            if !script.is_file() {
                bail!(
                    "menu not installed ({}); reinstall with scripts/install.sh --binary PATH",
                    script.display()
                );
            }
            let error = std::process::Command::new("bash")
                .arg(&script)
                .args(&args)
                .exec();
            bail!("could not start the menu: {error}")
        }
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
                ingress: String::new(),
                allow_non_loopback_ingress: false,
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
                baft: None,
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
                target: String::new(),
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
                baft: None,
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
                    instance,
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
                instance,
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
                    instance,
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
                    dir: dir.clone(),
                    instance: instance.clone(),
                },
                bundle_out.as_deref(),
            )?;
            println!("Key created (mode 0600, never exported).");
            if let Some(name) = &instance {
                for line in setup::adopt_for_service(&dir, &format!("{name}.json"))? {
                    println!("Owner/mode set for the service: {line}");
                }
            }
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
            // An instance configuration (`NAME.json`) is read by baft-tunnel@NAME as the service user.
            let file = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or_default();
            if file != "config.json" {
                for line in setup::adopt_for_service(&dir, file)? {
                    println!("Owner/mode set for the service: {line}");
                }
            }
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
        Command::AgentToken { out } => {
            agent::write_token(&out)?;
            println!("Token written: {}", out.display());
            Ok(())
        }
        Command::Agent {
            config: path,
            token_file,
            listen,
            state_dir,
            instance,
            allow_root,
        } => {
            // SAFETY: geteuid has no preconditions and cannot fail.
            if unsafe { libc::geteuid() } == 0 && !allow_root {
                anyhow::bail!(
                    "refusing to run the agent as root: run it as the baft-tunnel user (for example `setpriv --reuid=baft-tunnel --regid=baft-tunnel --init-groups baft-tunnel agent ...`), or pass --allow-root to accept the risk"
                );
            }
            let config = config::load(&path)?;
            // The agent runs as its own user and cannot read the tunnel's private key (SA-09);
            // the tunnel unit validates the key itself and fails closed.
            config.validate_public()?;
            let token = agent::read_token(&token_file)?;
            agent::run(
                config,
                agent::parse_listen(&listen)?,
                token,
                state_dir,
                instance,
            )
            .await
        }
        Command::Engine { command } => {
            match command {
                EngineCommand::Check {
                    manifest,
                    id,
                    binary,
                } => {
                    let sha = engine_store::check(&manifest, &id, &binary)?;
                    println!("engine {id}: binary matches pinned sha256 {sha}");
                }
                EngineCommand::Install {
                    store,
                    manifest,
                    id,
                    binary,
                } => {
                    let sha = engine_store::install(&store, &manifest, &id, &binary)?;
                    println!("engine {id}: installed and current = {sha}");
                }
                EngineCommand::Verify { store, id } => {
                    let bin = engine_store::verify_current(&store, &id)?;
                    println!("engine {id}: current verified: {}", bin.display());
                }
                EngineCommand::Rollback { store, id } => {
                    let sha = engine_store::rollback(&store, &id)?;
                    println!("engine {id}: rolled back, current = {sha}");
                }
            }
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
