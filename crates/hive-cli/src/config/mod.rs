//! Hive CLI configuration.

pub mod io;
mod validate;

pub use io::{load, save};
pub use validate::validate;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Top-level configuration file (.hive/config.toml).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Unique project identifier, used to scope Docker image/network names.
    /// Set by `hive init` and should not be changed manually.
    pub project_id: String,
    pub server: ServerConfig,
    /// List of agent definitions. Each entry becomes one agent container.
    pub agents: Vec<Agent>,
    pub app: AppConfig,
    pub exec: ExecConfig,
    pub logging: LoggingConfig,
}

/// A single agent definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// Unique name for this agent (used as the container name suffix).
    pub name: String,
    /// Coding agent binary: `kilo` or `claude`.
    pub coding_agent: String,
    /// Tags assigned to this agent.
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecConfig {
    /// Exact command aliases: key (e.g. "test") maps to a full command (e.g. "pnpm test").
    pub commands: HashMap<String, String>,
    /// Allowed command prefixes for `run <cmd>` (e.g. "cargo", "pnpm").
    pub run_prefixes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Port hive-server listens on inside Docker.
    pub port: u16,
    /// Host-side port exposed for hive-server.
    pub host_port: u16,
    /// Path to SQLite database file inside the container.
    pub db_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Port for the app-daemon HTTP server inside the container.
    pub daemon_port: u16,
    /// Host-side port exposed for the app-daemon.
    pub daemon_host_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: String,
}

// -- Defaults --

impl Default for Config {
    fn default() -> Self {
        Self {
            project_id: String::new(),
            server: ServerConfig::default(),
            agents: vec![],
            app: AppConfig::default(),
            exec: ExecConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            host_port: 8080,
            db_path: "/data/hive.db".to_string(),
        }
    }
}


impl Default for AppConfig {
    fn default() -> Self {
        Self {
            daemon_port: 8081,
            daemon_host_port: 8081,
        }
    }
}

impl Default for ExecConfig {
    fn default() -> Self {
        Self {
            commands: HashMap::from([
                ("test".to_string(), "pnpm test".to_string()),
                ("check".to_string(), "pnpm exec tsc --noEmit".to_string()),
                ("build".to_string(), "pnpm build".to_string()),
            ]),
            run_prefixes: vec![
                "cargo".to_string(),
                "npm".to_string(),
                "pnpm".to_string(),
            ],
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
        }
    }
}
