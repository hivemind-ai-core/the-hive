//! `hive init` — initialize a project for use with The Hive.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::config::{self, Agent};
use crate::config::io::{hive_dir, default_path};
use crate::install;
use crate::tui;

/// Run `hive init` in the given project directory.
pub fn run(project_dir: &Path) -> Result<()> {
    let hive = hive_dir(project_dir);
    let config_path = default_path(project_dir);

    // Create .hive/ directory.
    std::fs::create_dir_all(&hive)
        .with_context(|| format!("creating {}", hive.display()))?;

    println!("Initializing Hive in {}", hive.display());

    // Require hive to be installed first.
    if !install::is_installed() {
        bail!(
            "hive is not installed. Run 'hive install' first.\n\
             (Expected: {})",
            install::hive_bin_dir().join("hive-server").display()
        );
    }

    // Load or create config (run wizard if new).
    let existing = config::load(&config_path)?;
    let mut cfg = if existing.project_id.is_empty() {
        // New project: pre-populate one default agent then open wizard.
        let mut seed = existing;
        if seed.agents.is_empty() {
            seed.agents.push(Agent {
                name: "kilo-1".to_string(),
                coding_agent: "kilo".to_string(),
                tags: vec![],
                env: Default::default(),
            });
        }
        tui::config::run_wizard(seed)?
    } else {
        println!("Config already exists — skipping wizard. Edit {} to change settings.", config_path.display());
        existing
    };

    // Generate project ID if not set.
    if cfg.project_id.is_empty() {
        cfg.project_id = generate_project_id(project_dir);
        println!("Project ID: {}", cfg.project_id);
    }

    // Validate config.
    config::validate(&cfg)?;

    // Write config.
    config::save(&cfg, &config_path)?;
    println!("Created {}", config_path.display());

    // Copy Dockerfiles from ~/.hive/docker/ (skip if already exist — user may have customized).
    let docker_src = install::hive_docker_dir();
    for name in &["Dockerfile.server", "Dockerfile.agent", "Dockerfile.app"] {
        let src = docker_src.join(name);
        let dst = hive.join(name);
        let content = std::fs::read_to_string(&src)
            .with_context(|| format!("reading {} from {}", name, src.display()))?;
        write_if_new(&dst, &content, "Created")?;
    }

    // Copy pre-built binaries from ~/.hive/bin/ → .hive/bin/ (always overwrite).
    let bin_dst = hive.join("bin");
    std::fs::create_dir_all(&bin_dst)
        .with_context(|| format!("creating {}", bin_dst.display()))?;

    for name in &["hive-server", "hive-agent", "app-daemon"] {
        let src = install::container_binary(name);
        if !src.exists() {
            bail!(
                "{} not found.\nRun 'hive install' to populate ~/.hive/.",
                src.display()
            );
        }
        let dst = bin_dst.join(name);
        std::fs::copy(&src, &dst)
            .with_context(|| format!("copying {name} to .hive/bin/"))?;
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755))
            .with_context(|| format!("setting permissions on {name}"))?;
        println!("Copied {name} → .hive/bin/{name}");
    }

    // Update .gitignore to exclude hive.db and .hive/bin/.
    update_gitignore(project_dir)?;

    println!("\nRun 'hive start' to build images and launch the hive.");
    Ok(())
}

fn write_if_new(path: &Path, content: &str, verb: &str) -> Result<()> {
    if path.exists() {
        println!("Skipped {} (already exists)", path.display());
    } else {
        std::fs::write(path, content)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("{} {}", verb, path.display());
    }
    Ok(())
}

fn generate_project_id(project_dir: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let name = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf())
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>();

    // 4-char hex suffix from hash of the absolute path.
    let mut hasher = DefaultHasher::new();
    project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf())
        .hash(&mut hasher);
    let suffix = format!("{:04x}", hasher.finish() & 0xffff);

    format!("{name}-{suffix}")
}

fn update_gitignore(project_dir: &Path) -> Result<()> {
    let gitignore = project_dir.join(".gitignore");
    let entries = [".hive/hive.db", ".hive/bin/", ".hive/.env"];

    let existing = if gitignore.exists() {
        std::fs::read_to_string(&gitignore).context("reading .gitignore")?
    } else {
        String::new()
    };

    let mut updated = existing.trim_end().to_string();
    let mut changed = false;

    for entry in &entries {
        if !existing.lines().any(|l| l.trim() == *entry) {
            updated.push('\n');
            updated.push_str(entry);
            changed = true;
        }
    }

    if changed {
        updated.push('\n');
        std::fs::write(&gitignore, &updated).context("updating .gitignore")?;
        println!("Updated .gitignore");
    }

    Ok(())
}
