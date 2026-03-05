//! Canonical layout of the hive installation directory and helper functions.
//!
//! Layout:
//! ```
//! ~/.hive/
//!   bin/
//!     hive           # CLI binary
//!     hive-server    # Linux AMD64 binary (for Docker containers)
//!     hive-agent     # Linux AMD64 binary (for Docker containers)
//!     app-daemon     # Linux AMD64 binary (for Docker containers)
//!   docker/
//!     Dockerfile.server
//!     Dockerfile.agent
//!     Dockerfile.app
//!   version          # plain text: "0.1.3"
//! ```

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Returns the hive home directory.
///
/// Respects the `HIVE_HOME` environment variable as an override.
/// Falls back to `~/.hive`.
pub fn hive_home() -> PathBuf {
    if let Ok(override_path) = std::env::var("HIVE_HOME") {
        return PathBuf::from(override_path);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hive")
}

/// Returns `~/.hive/bin`.
pub fn hive_bin_dir() -> PathBuf {
    hive_home().join("bin")
}

/// Returns `~/.hive/docker`.
pub fn hive_docker_dir() -> PathBuf {
    hive_home().join("docker")
}

/// Returns `~/.hive/version`.
pub fn hive_version_file() -> PathBuf {
    hive_home().join("version")
}

/// Reads `~/.hive/version`. Returns `None` if not present or not readable.
pub fn installed_version() -> Option<String> {
    std::fs::read_to_string(hive_version_file())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Returns `true` if the hive-server binary and Dockerfile.server both exist,
/// indicating that the installation is present.
pub fn is_installed() -> bool {
    hive_bin_dir().join("hive-server").exists()
        && hive_docker_dir().join("Dockerfile.server").exists()
}

/// Creates `~/.hive/bin/` and `~/.hive/docker/` if they don't already exist.
pub fn ensure_dirs() -> Result<()> {
    let bin = hive_bin_dir();
    let docker = hive_docker_dir();
    std::fs::create_dir_all(&bin)
        .with_context(|| format!("creating {}", bin.display()))?;
    std::fs::create_dir_all(&docker)
        .with_context(|| format!("creating {}", docker.display()))?;
    Ok(())
}

/// Returns the path to a named Linux AMD64 binary inside `~/.hive/bin/`.
///
/// Example: `container_binary("hive-server")` → `~/.hive/bin/hive-server`
pub fn container_binary(name: &str) -> PathBuf {
    hive_bin_dir().join(name)
}
