//! Config file I/O.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::{Config, GlobalConfig, CONFIG_VERSION, migrate_config};

/// Default config file path: `<project_dir>/.hive/config.toml`
pub fn default_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".hive").join("config.toml")
}

/// Path to the `.hive/` directory.
pub fn hive_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".hive")
}

/// Load config from file. Returns `Config::default()` if the file does not exist.
///
/// Automatically migrates configs with an older `version` to the current format.
pub fn load(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading config from {:?}", path))?;
    let cfg: Config = toml::from_str(&raw).with_context(|| format!("parsing config {:?}", path))?;
    if cfg.version < CONFIG_VERSION {
        Ok(migrate_config(cfg))
    } else {
        Ok(cfg)
    }
}

/// Write config to file, creating parent directories as needed.
pub fn save(config: &Config, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {:?}", parent))?;
    }
    let raw = toml::to_string_pretty(config).context("serializing config")?;
    std::fs::write(path, raw).with_context(|| format!("writing config to {:?}", path))?;
    Ok(())
}

// -- Global config --

/// Path to the global config file (`~/.config/hive/config.toml`).
pub fn global_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"))
        .join("hive")
        .join("config.toml")
}

/// Load global config. Returns `GlobalConfig::default()` if the file does not exist.
pub fn load_global() -> GlobalConfig {
    let path = global_config_path();
    if !path.exists() {
        return GlobalConfig::default();
    }
    match std::fs::read_to_string(&path)
        .context("")
        .and_then(|raw| toml::from_str(&raw).context(""))
    {
        Ok(cfg) => cfg,
        Err(_) => GlobalConfig::default(),
    }
}

/// Write global config to its standard path, creating parent dirs as needed.
pub fn save_global(config: &GlobalConfig) -> Result<()> {
    let path = global_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {:?}", parent))?;
    }
    let raw = toml::to_string_pretty(config).context("serializing global config")?;
    std::fs::write(&path, raw).with_context(|| format!("writing global config to {:?}", path))?;
    Ok(())
}
