//! Config validation.

use anyhow::{bail, Result};

use super::Config;

const VALID_CODING_AGENTS: &[&str] = &["kilo", "claude"];
const VALID_LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];

/// Validate a loaded config and return a descriptive error if anything is wrong.
pub fn validate(config: &Config) -> Result<()> {
    // Agents vector
    if config.agents.is_empty() {
        bail!("agents must have at least 1 entry");
    }
    if config.agents.len() > 10 {
        bail!("agents must have 10 or fewer entries");
    }
    for (i, agent) in config.agents.iter().enumerate() {
        if agent.name.trim().is_empty() {
            bail!("agents[{i}].name must not be empty");
        }
        if !VALID_CODING_AGENTS.contains(&agent.coding_agent.as_str()) {
            bail!(
                "agents[{i}].coding_agent must be one of: {}",
                VALID_CODING_AGENTS.join(", ")
            );
        }
    }

    // Ports
    validate_port("server.port", config.server.port)?;
    validate_port("server.host_port", config.server.host_port)?;
    validate_port("app.daemon_port", config.app.daemon_port)?;
    validate_port("app.daemon_host_port", config.app.daemon_host_port)?;

    // Project ID (set by hive init)
    if config.project_id.trim().is_empty() {
        bail!("project_id is not set — run 'hive init' first");
    }

    // Log level
    if !VALID_LOG_LEVELS.contains(&config.logging.level.as_str()) {
        bail!(
            "logging.level must be one of: {}",
            VALID_LOG_LEVELS.join(", ")
        );
    }

    Ok(())
}

fn validate_port(name: &str, port: u16) -> Result<()> {
    if port == 0 {
        bail!("{name} must be a valid port (1-65535)");
    }
    Ok(())
}
