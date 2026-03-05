//! Hive CLI configuration.

pub mod io;
mod validate;

pub use io::{load, save, load_global, save_global, global_config_path};
pub use validate::validate;

/// Current config file format version. Bump when making breaking schema changes.
pub const CONFIG_VERSION: u32 = 1;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Top-level configuration file (.hive/config.toml).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Config format version. Used to run migrations when the format changes.
    /// Absent in old configs (defaults to 0).
    pub version: u32,
    /// Unique project identifier, used to scope Docker image/network names.
    /// Set by `hive init` and should not be changed manually.
    pub project_id: String,
    pub server: ServerConfig,
    /// List of agent definitions. Each entry becomes one agent container.
    pub agents: Vec<Agent>,
    pub app: AppConfig,
    pub exec: ExecConfig,
    pub logging: LoggingConfig,
    /// Network isolation settings.
    pub network: NetworkConfig,
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
    /// Extra environment variables passed to this agent's container.
    /// Merged on top of any project-wide .hive/.env values.
    #[serde(default)]
    pub env: HashMap<String, String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    /// Block agent containers from reaching the internet (internal Docker network).
    /// Set to false to allow agents to make outbound HTTP requests.
    /// Default: true.
    pub isolate: bool,
}

// -- Defaults --

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            project_id: String::new(),
            server: ServerConfig::default(),
            agents: vec![],
            app: AppConfig::default(),
            exec: ExecConfig::default(),
            logging: LoggingConfig::default(),
            network: NetworkConfig::default(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self { isolate: true }
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

// -- Config migration --

/// Migrate config from an older version to the current one.
///
/// Called automatically by `config::load` when `config.version < CONFIG_VERSION`.
/// Add version-specific migration steps inside the match arms as the format evolves.
pub fn migrate_config(mut config: Config) -> Config {
    // Walk through each version step so migrations compose correctly.
    #[allow(clippy::match_single_binding)]
    match config.version {
        0 => {
            // v0 → v1: no structural changes; just stamp the version.
        }
        _ => {} // Already up-to-date or unknown future version.
    }
    config.version = CONFIG_VERSION;
    config
}

// -- Global config (~/.config/hive/config.toml) --

/// Global configuration applied as defaults across all projects.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GlobalConfig {
    pub defaults: GlobalDefaults,
    pub docker: GlobalDockerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalDefaults {
    /// Default number of agents for new projects.
    pub agents: usize,
}

impl Default for GlobalDefaults {
    fn default() -> Self {
        Self { agents: 2 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GlobalDockerConfig {
    /// Docker socket URI (e.g. "unix:///var/run/docker.sock").
    /// Overrides DOCKER_HOST if set.
    pub socket: Option<String>,
}
