//! Hive CLI - User-facing command-line interface for The Hive.
//!
//! Provides commands for managing Docker containers, interacting with
//! the swarm, and a TUI for monitoring.

use clap::Parser;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "hive")]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, global = true, help = "Verbose logging")]
    verbose: bool,

    #[arg(short, long, global = true, help = "Project directory")]
    directory: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Parser, Debug)]
enum Commands {
    /// Start all containers (hive-server, app-container, hive-agent[N])
    Start,
    /// Stop all containers
    Stop,
    /// Restart all containers
    Restart,
    /// Open the TUI
    Ui,
    /// Show container status
    Status,
    /// Manage configuration
    Config,
    /// Show logs
    Logs,
    /// Manage agents
    Agent,
    /// Task commands
    Task,
    /// Message board commands
    Topic,
}

fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    if args.verbose {
        info!("Verbose mode enabled");
    }

    // TODO: Implement CLI commands
    println!("Hive CLI - TODO: Implement commands");
    println!("Run 'hive --help' for usage information");
}
