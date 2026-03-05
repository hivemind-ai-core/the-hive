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
        /// Container name (hive-server, hive-app, hive-agent-N)
        #[arg(default_value = "hive-server")]
        container: String,
    },
    /// Interactive configuration setup
    Config,
    /// Open the TUI monitor
    Ui,
    /// Check for and apply updates from GitHub releases
    Update {
        /// Only check the latest version without downloading
        #[arg(long)]
        check: bool,
    },
    /// Manage agent authentication (Claude OAuth / API keys)
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
}

#[derive(Subcommand, Debug)]
enum AuthAction {
    /// Copy ~/.claude.json to .hive/claude.json for use in agent containers
    Sync,
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
        Commands::Logs { container } => commands::logs(&args.directory, &container).await?,
        Commands::Config => {
            let path = config::io::default_path(&args.directory);
            let existing = config::load(&path)?;
            let updated = tui::config::run_wizard(existing)?;
            config::validate(&updated)?;
            config::save(&updated, &path)?;
            println!("Configuration saved to {}", path.display());
        }
        Commands::Ui => {
            let cfg = config::load(&config::io::default_path(&args.directory))?;
            let server_url = format!("ws://localhost:{}/ws", cfg.server.host_port);
            tui::app::run(server_url, args.directory.clone(), cfg)?;
        }
        Commands::Update { check } => updater::run(check).await?,
        Commands::Auth { action } => match action {
            AuthAction::Sync => commands::auth_sync(&args.directory)?,
            AuthAction::Login { email } => commands::auth_login(&args.directory, email.as_deref()).await?,
        },
    }

    Ok(())
}
