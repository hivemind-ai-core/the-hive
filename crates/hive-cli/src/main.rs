//! Hive CLI - User-facing command-line interface for The Hive.
//!
//! Provides commands for managing Docker containers, interacting with
//! the swarm, and a TUI for monitoring.

mod commands;
mod config;
mod docker;
mod init;
mod install;
mod tui;
mod updater;
mod version;

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "hive")]
#[command(version, about = "Manage The Hive multi-agent system", long_about = None)]
struct Args {
    #[arg(short, long, global = true, help = "Verbose logging")]
    verbose: bool,

    #[arg(short, long, global = true, help = "Project directory", default_value = ".")]
    directory: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize hive in the current project (creates .hive/ with config and Dockerfiles)
    Init,
    /// Start all containers (runs init if needed, builds images if not present)
    Start,
    /// Stop all containers
    Stop {
        /// Also remove containers (next start will recreate them)
        #[arg(short, long)]
        remove: bool,
    },
    /// Restart all containers
    Restart,
    /// Rebuild Docker images from .hive/Dockerfiles
    Rebuild {
        /// Which image to rebuild: server, agent, app (default: all)
        #[arg(default_value = "all")]
        target: String,
    },
    /// Show container status
    Status,
    /// Show container logs
    Logs {
        /// Container alias: all, server, app, or agent name (default: all)
        #[arg(default_value = "all")]
        container: String,
        /// Follow (stream) logs continuously
        #[arg(short, long)]
        follow: bool,
    },
    /// Interactive configuration setup
    Config {
        /// Edit the global config (~/.config/hive/config.toml) instead of project config
        #[arg(long)]
        global: bool,
    },
    /// Open the TUI monitor
    Ui,
    /// Install (or reinstall) companion binaries and Docker templates into ~/.hive/
    Install,
    /// Check for and apply updates from GitHub releases
    Update {
        /// Only check the latest version without downloading
        #[arg(long)]
        check: bool,
    },
    /// Manage agent authentication (Claude OAuth / API keys)
    Auth {
        #[command(subcommand)]
        action: Option<AuthAction>,
    },
}

#[derive(Subcommand, Debug)]
enum AuthAction {
    /// Show current auth configuration and detected credentials
    Status,
    /// Write an API key to .hive/.env or a specific agent's env block in config.toml
    SetKey {
        /// Variable name (e.g. `ANTHROPIC_API_KEY`)
        key: String,
        /// Key value
        value: String,
        /// Write to a specific agent's env block instead of the shared .hive/.env
        #[arg(long)]
        agent: Option<String>,
    },
    /// Write a base URL to .hive/.env or a specific agent's env block in config.toml
    SetEndpoint {
        /// Variable name (e.g. `OPENAI_BASE_URL`)
        key: String,
        /// Base URL (e.g. <https://api.together.xyz/v1>)
        url: String,
        /// Write to a specific agent's env block instead of the shared .hive/.env
        #[arg(long)]
        agent: Option<String>,
    },
    /// List all keys and endpoints in .hive/.env (values masked)
    List,
    /// Copy ~/.claude.json to .hive/claude.json for use in agent containers
    Sync,
    /// Copy ~/.kilocode/ to .hive/kilocode[-{agent}]/ for project-local kilo config
    KiloSync {
        /// Sync for a specific agent only (writes to .hive/kilocode-{name}/)
        #[arg(long)]
        agent: Option<String>,
    },
    /// Run 'claude auth login' inside the agent container (for OAuth/subscription users)
    Login {
        /// Email address for the Claude account
        #[arg(long)]
        email: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let log_level = if args.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .init();

    match args.command {
        Commands::Init => init::run(&args.directory)?,
        Commands::Start => commands::start(&args.directory).await?,
        Commands::Stop { remove } => commands::stop(&args.directory, remove).await?,
        Commands::Restart => commands::restart(&args.directory).await?,
        Commands::Rebuild { target } => commands::rebuild(&args.directory, &target).await?,
        Commands::Status => commands::status(&args.directory).await?,
        Commands::Logs { container, follow } => commands::logs(&args.directory, &container, follow).await?,
        Commands::Config { global } => {
            if global {
                let path = config::global_config_path();
                // Ensure the file exists with defaults before opening.
                if !path.exists() {
                    config::save_global(&config::GlobalConfig::default())?;
                    println!("Created {}", path.display());
                }
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                std::process::Command::new(&editor)
                    .arg(&path)
                    .status()
                    .with_context(|| format!("opening {editor}"))?;
            } else {
                let path = config::io::default_path(&args.directory);
                let existing = config::load(&path)?;
                let updated = tui::config::run_wizard(existing, args.directory.clone())?;
                config::validate(&updated)?;
                config::save(&updated, &path)?;
                println!("Configuration saved to {}", path.display());
            }
        }
        Commands::Ui => {
            let cfg = config::load(&config::io::default_path(&args.directory))?;
            let server_url = format!("ws://localhost:{}/ws", cfg.server.host_port);
            tui::app::run(server_url, args.directory.clone(), cfg)?;
        }
        Commands::Install => updater::run(false).await?,
        Commands::Update { check } => updater::run(check).await?,
        Commands::Auth { action } => match action {
            None => {
                println!("Hive Auth — manage agent credentials");
                println!();
                println!("For kilo agents:");
                println!("  hive auth set-key ANTHROPIC_API_KEY sk-ant-...");
                println!();
                println!("For claude agents (choose one):");
                println!("  hive auth set-key ANTHROPIC_API_KEY sk-ant-...   # API key");
                println!("  hive auth sync                                    # copy ~/.claude.json");
                println!("  hive auth login                                   # interactive login");
                println!();
                println!("Current status:");
                commands::auth_status(&args.directory)?;
                println!();
                println!("Subcommands: set-key, set-endpoint, list, status, sync, kilo-sync, login");
            }
            Some(AuthAction::Status) => commands::auth_status(&args.directory)?,
            Some(AuthAction::SetKey { key, value, agent }) => commands::auth_set_key(&args.directory, &key, &value, agent.as_deref())?,
            Some(AuthAction::SetEndpoint { key, url, agent }) => commands::auth_set_endpoint(&args.directory, &key, &url, agent.as_deref())?,
            Some(AuthAction::List) => commands::auth_list(&args.directory)?,
            Some(AuthAction::Sync) => commands::auth_sync(&args.directory)?,
            Some(AuthAction::KiloSync { agent }) => commands::auth_kilo_sync(&args.directory, agent.as_deref())?,
            Some(AuthAction::Login { email }) => commands::auth_login(&args.directory, email.as_deref()).await?,
        },
    }

    Ok(())
}
