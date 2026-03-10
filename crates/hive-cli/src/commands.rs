//! CLI command implementations.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use bollard::Docker;
use tokio::process::Command;
use tracing::info;

use crate::config::io::{default_path, hive_dir};
use crate::config::{self, Config};
use crate::docker::{containers, lifecycle, network};

fn connect_docker() -> Result<Docker> {
    let gcfg = config::load_global();
    if let Some(socket) = gcfg.docker.socket.as_deref() {
        // Use socket URI from global config.
        Docker::connect_with_socket(socket, 120, bollard::API_DEFAULT_VERSION)
            .context("connecting to Docker via global config socket")
    } else {
        Docker::connect_with_local_defaults().context("connecting to Docker")
    }
}

fn load_config(project_dir: &Path) -> Result<Config> {
    let path = default_path(project_dir);
    let cfg = config::load(&path)?;
    config::validate(&cfg)?;
    Ok(cfg)
}

/// Ensure all three project images exist, building any that are missing.
async fn ensure_images(docker: &Docker, cfg: &Config, project_dir: &Path) -> Result<()> {
    let id = &cfg.project_id;
    let hive = hive_dir(project_dir);

    let builds = [
        (containers::server_image(id), "Dockerfile.server", "server"),
        (containers::agent_image(id), "Dockerfile.agent", "agent"),
        (containers::app_image(id), "Dockerfile.app", "app"),
    ];

    for (image, dockerfile, label) in &builds {
        if containers::image_exists(docker, image).await? {
            info!("Image '{image}' already exists");
            continue;
        }
        let dockerfile_path = hive.join(dockerfile);
        anyhow::ensure!(
            dockerfile_path.exists(),
            "Dockerfile not found: {}\nRun 'hive init' first.",
            dockerfile_path.display()
        );
        println!("Building {label} image ({image})...");
        let status = Command::new("docker")
            .args(["build", "-t", image, "-f"])
            .arg(&dockerfile_path)
            .arg(project_dir)
            .status()
            .await
            .with_context(|| format!("running docker build for {label}"))?;
        anyhow::ensure!(status.success(), "docker build failed for {label}");
        println!("Built {image}");
    }
    Ok(())
}

/// `hive start` — init if needed, build images if needed, create and start containers.
pub async fn start(project_dir: &Path) -> Result<()> {
    let config_path = default_path(project_dir);

    // Auto-run init if not yet initialized.
    if !config_path.exists() {
        println!("No .hive/config.toml found — running 'hive init' first.\n");
        crate::init::run(project_dir)?;
    }

    let cfg = load_config(project_dir)?;
    let docker = connect_docker()?;
    let id = &cfg.project_id;

    if cfg.network.isolate {
        println!();
        println!("WARNING: Agent network is isolated (network.isolate = true).");
        println!("  Agents cannot reach external provider APIs (Anthropic, OpenAI, etc.).");
        println!("  To allow provider access, set network.isolate = false in .hive/config.toml.");
        println!("  For partial isolation, point ANTHROPIC_BASE_URL / OPENAI_BASE_URL at an egress proxy.");
        println!();
    }

    // External network: server + app-daemon. Non-internal so host can reach published ports.
    network::ensure(&docker, id).await?;
    // Agent network: agents + server. Internal if isolation enabled.
    network::ensure_agent_network(&docker, id, cfg.network.isolate).await?;

    // Build any missing images.
    ensure_images(&docker, &cfg, project_dir).await?;

    let server = containers::server_name(id);
    let app = containers::app_name(id);

    // Create containers only if they don't already exist (first time or after rebuild).
    for r in [
        containers::create_server(&docker, &cfg, project_dir).await,
        containers::create_app(&docker, &cfg, project_dir).await,
    ] {
        if let Err(e) = r {
            if !format!("{e:#}").contains("already in use") {
                return Err(e);
            }
        }
    }
    containers::create_agents(&docker, &cfg, project_dir).await?;

    // Connect server to the agent network so agents can reach it by container name.
    // Ignores "already connected" errors (container may have been created on a prior start).
    if let Err(e) =
        containers::connect_to_network(&docker, &server, &network::agent_network_name(id)).await
    {
        if !format!("{e:#}").contains("already exists") {
            return Err(e);
        }
    }

    // Start in order: server → app → agents.
    lifecycle::start(&docker, &server).await?;
    lifecycle::wait_healthy(&docker, &server, Duration::from_secs(30)).await?;

    lifecycle::start(&docker, &app).await?;

    for agent in &cfg.agents {
        lifecycle::start(&docker, &containers::agent_name(id, &agent.name)).await?;
    }

    println!("All containers started. Run 'hive ui' to open the TUI.");
    warn_missing_credentials(&cfg, project_dir);
    Ok(())
}

/// `hive stop [--remove]` — stop all containers, optionally removing them.
pub async fn stop(project_dir: &Path, remove: bool) -> Result<()> {
    let cfg = load_config(project_dir)?;
    let docker = connect_docker()?;
    let id = &cfg.project_id;

    // Build the set of known agent container names.
    let known: Vec<String> = cfg
        .agents
        .iter()
        .map(|a| containers::agent_name(id, &a.name))
        .collect();

    // Find agent containers that are no longer in the config (orphans).
    let orphans = containers::orphaned_agent_names(&docker, id, &known)
        .await
        .unwrap_or_default();

    for agent in cfg.agents.iter().rev() {
        lifecycle::stop(&docker, &containers::agent_name(id, &agent.name)).await?;
    }
    for orphan in &orphans {
        lifecycle::stop(&docker, orphan).await?;
    }
    lifecycle::stop(&docker, &containers::app_name(id)).await?;
    lifecycle::stop(&docker, &containers::server_name(id)).await?;

    if remove {
        for agent in cfg.agents.iter().rev() {
            lifecycle::remove(&docker, &containers::agent_name(id, &agent.name)).await?;
        }
        for orphan in &orphans {
            lifecycle::remove(&docker, orphan).await?;
        }
        lifecycle::remove(&docker, &containers::app_name(id)).await?;
        lifecycle::remove(&docker, &containers::server_name(id)).await?;
        network::remove_all(&docker, id).await?;
        info!("All containers stopped and removed");
    } else {
        info!("All containers stopped");
    }
    Ok(())
}

/// `hive restart` — stop, remove, and recreate all containers to pick up config changes.
///
/// This always recreates containers so that env var changes, tag changes, mount changes,
/// and other config modifications in `.hive/config.toml` take effect immediately.
/// Volume data (`.hive/`, project dir) persists across recreation.
pub async fn restart(project_dir: &Path) -> Result<()> {
    println!("Stopping and removing containers (will recreate with current config)…");
    stop(project_dir, true).await?;
    start(project_dir).await
}

/// Sync container binaries from ~/.hive/bin/ → .hive/bin/ so Docker builds pick up fresh builds.
fn sync_bins(project_dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let bin_dst = hive_dir(project_dir).join("bin");
    std::fs::create_dir_all(&bin_dst).context("creating .hive/bin/")?;
    for name in &["hive-server", "hive-agent", "app-daemon"] {
        let src = crate::install::container_binary(name);
        anyhow::ensure!(
            src.exists(),
            "{} not found — run 'just install' first",
            src.display()
        );
        let dst = bin_dst.join(name);
        std::fs::copy(&src, &dst).with_context(|| format!("syncing {name} to .hive/bin/"))?;
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755))
            .with_context(|| format!("setting permissions on {name}"))?;
    }
    Ok(())
}

/// `hive rebuild [target]` — rebuild Docker images and replace running containers.
pub async fn rebuild(project_dir: &Path, target: &str) -> Result<()> {
    let cfg = load_config(project_dir)?;
    let id = &cfg.project_id;
    let hive = hive_dir(project_dir);
    let docker = connect_docker()?;

    sync_bins(project_dir)?;

    let all = [
        (
            "server",
            "Dockerfile.server",
            containers::server_image(id),
            containers::server_name(id),
        ),
        (
            "agent",
            "Dockerfile.agent",
            containers::agent_image(id),
            String::new(),
        ), // image-only target
        (
            "app",
            "Dockerfile.app",
            containers::app_image(id),
            containers::app_name(id),
        ),
    ];

    let builds: Vec<_> = if target == "all" {
        all.iter().collect()
    } else {
        all.iter()
            .filter(|(name, _, _, _)| *name == target)
            .collect()
    };

    anyhow::ensure!(
        !builds.is_empty(),
        "Unknown target '{target}'. Use: server, agent, app, all"
    );

    // Build new images.
    for (name, dockerfile, tag, _) in &builds {
        let dockerfile_path = hive.join(dockerfile);
        anyhow::ensure!(
            dockerfile_path.exists(),
            "Dockerfile not found: {}",
            dockerfile_path.display()
        );

        println!("Building {name} → {tag}");
        let status = Command::new("docker")
            .args(["build", "-t", tag.as_str(), "-f"])
            .arg(&dockerfile_path)
            .arg(project_dir)
            .status()
            .await
            .with_context(|| format!("running docker build for {name}"))?;

        anyhow::ensure!(status.success(), "docker build failed for {name}");
        println!("Built {tag}");
    }

    // Remove old containers for rebuilt targets so they pick up the new image on next start.
    for (name, _, _, _) in &builds {
        match *name {
            "server" => lifecycle::remove(&docker, &containers::server_name(id)).await?,
            "app" => lifecycle::remove(&docker, &containers::app_name(id)).await?,
            "agent" => {
                for agent in cfg.agents.iter().rev() {
                    lifecycle::remove(&docker, &containers::agent_name(id, &agent.name)).await?;
                }
            }
            _ => {}
        }
    }

    println!("Done. Run 'hive start' to launch with the new images.");
    Ok(())
}

/// `hive status` — print container states.
pub async fn status(project_dir: &Path) -> Result<()> {
    let cfg = load_config(project_dir)?;
    let docker = connect_docker()?;
    let id = &cfg.project_id;

    let mut names = vec![containers::server_name(id), containers::app_name(id)];
    for agent in &cfg.agents {
        names.push(containers::agent_name(id, &agent.name));
    }

    println!("{:<35} {:<15} ID", "CONTAINER", "STATUS");
    println!("{}", "-".repeat(65));

    for name in &names {
        match docker.inspect_container(name, None).await {
            Ok(info) => {
                let status = info
                    .state
                    .as_ref()
                    .and_then(|s| s.status.as_ref())
                    .map_or_else(|| "unknown".to_string(), |s| format!("{s:?}"));
                let cid = info.id.as_deref().unwrap_or("-");
                println!("{:<35} {:<15} {}", name, status, &cid[..8.min(cid.len())]);
            }
            Err(_) => {
                println!("{:<35} {:<15} -", name, "not found");
            }
        }
    }
    Ok(())
}

/// Write or update a single `KEY=VALUE` entry in `.hive/.env`.
fn dotenv_set(path: &std::path::Path, key: &str, value: &str) -> Result<()> {
    let existing = if path.exists() {
        std::fs::read_to_string(path).context("reading .hive/.env")?
    } else {
        String::new()
    };

    let mut lines: Vec<String> = existing.lines().map(str::to_owned).collect();
    let prefix = format!("{key}=");
    let new_line = format!("{key}={value}");

    if let Some(pos) = lines.iter().position(|l| l.starts_with(&prefix)) {
        lines[pos] = new_line;
    } else {
        lines.push(new_line);
    }

    // Ensure trailing newline.
    let mut content = lines.join("\n");
    content.push('\n');

    std::fs::write(path, &content).context("writing .hive/.env")?;
    Ok(())
}

/// `hive auth set-key KEY VALUE [--agent NAME]` — write an API key.
///
/// Without `--agent`: writes to `.hive/.env` (shared across all agents).
/// With `--agent NAME`: writes to that agent's `env:` block in `config.toml`.
pub fn auth_set_key(project_dir: &Path, key: &str, value: &str, agent: Option<&str>) -> Result<()> {
    anyhow::ensure!(!key.is_empty(), "key must not be empty");
    anyhow::ensure!(!value.is_empty(), "value must not be empty");

    let masked = if value.len() > 8 {
        format!("{}***", &value[..8])
    } else {
        "***".to_string()
    };

    if let Some(name) = agent {
        let config_path = default_path(project_dir);
        let mut cfg = config::load(&config_path)?;
        let agent = cfg
            .agents
            .iter_mut()
            .find(|a| a.name == name)
            .ok_or_else(|| anyhow::anyhow!("agent '{}' not found in config", name))?;
        agent.env.insert(key.to_string(), value.to_string());
        config::save(&cfg, &config_path)?;
        println!("Set {key}={masked} in agent '{name}' env (config.toml)");
    } else {
        let env_path = hive_dir(project_dir).join(".env");
        dotenv_set(&env_path, key, value)?;
        println!("Set {key}={masked} in .hive/.env");
    }
    println!("Run 'hive restart' to apply to running containers.");
    Ok(())
}

/// `hive auth set-endpoint KEY URL [--agent NAME]` — write a base URL.
///
/// Without `--agent`: writes to `.hive/.env` (shared across all agents).
/// With `--agent NAME`: writes to that agent's `env:` block in `config.toml`.
pub fn auth_set_endpoint(
    project_dir: &Path,
    key: &str,
    url: &str,
    agent: Option<&str>,
) -> Result<()> {
    anyhow::ensure!(!key.is_empty(), "key must not be empty");
    anyhow::ensure!(!url.is_empty(), "url must not be empty");

    if let Some(name) = agent {
        let config_path = default_path(project_dir);
        let mut cfg = config::load(&config_path)?;
        let agent = cfg
            .agents
            .iter_mut()
            .find(|a| a.name == name)
            .ok_or_else(|| anyhow::anyhow!("agent '{}' not found in config", name))?;
        agent.env.insert(key.to_string(), url.to_string());
        config::save(&cfg, &config_path)?;
        println!("Set {key}={url} in agent '{name}' env (config.toml)");
    } else {
        let env_path = hive_dir(project_dir).join(".env");
        dotenv_set(&env_path, key, url)?;
        println!("Set {key}={url} in .hive/.env");
    }
    println!("Run 'hive restart' to apply to running containers.");
    Ok(())
}

/// `hive auth list` — list all keys/endpoints in `.hive/.env` with masked values.
pub fn auth_list(project_dir: &Path) -> Result<()> {
    let env_path = hive_dir(project_dir).join(".env");

    if !env_path.exists() {
        println!(".hive/.env not found. Use 'hive auth set-key' to add credentials.");
        return Ok(());
    }

    let content = std::fs::read_to_string(&env_path).context("reading .hive/.env")?;
    let entries: Vec<_> = content
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();

    if entries.is_empty() {
        println!(".hive/.env is empty.");
        return Ok(());
    }

    println!("Contents of .hive/.env:");
    for line in entries {
        if let Some((k, v)) = line.split_once('=') {
            // Show URLs in full; mask key-like values.
            let display = if v.starts_with("http://") || v.starts_with("https://") {
                v.to_string()
            } else if v.len() > 8 {
                format!("{}***", &v[..8])
            } else {
                "***".to_string()
            };
            println!("  {k}={display}");
        }
    }
    Ok(())
}

/// Check each agent for missing credentials and print actionable warnings.
/// Called after `hive start` to alert operators without blocking startup.
fn warn_missing_credentials(cfg: &Config, project_dir: &Path) {
    let hive = hive_dir(project_dir);
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/root"));

    let dotenv_path = hive.join(".env");
    let dotenv_content = if dotenv_path.exists() {
        std::fs::read_to_string(&dotenv_path).unwrap_or_default()
    } else {
        String::new()
    };
    let has_key = |key: &str| {
        dotenv_content.lines().any(|l| {
            l.trim_start().starts_with(key) && l.contains('=') && {
                let v = l.split_once('=').map_or("", |(_, r)| r).trim();
                !v.is_empty()
            }
        })
    };

    let claude_json_host = home.join(".claude.json");
    let claude_json_hive = hive.join("claude.json");
    let kilocode_dir_host = home.join(".kilocode");

    for agent in &cfg.agents {
        let missing = match agent.coding_agent.as_str() {
            "claude" => {
                !claude_json_host.exists()
                    && !claude_json_hive.exists()
                    && !has_key("ANTHROPIC_API_KEY")
            }
            "kilo" => {
                !kilocode_dir_host.exists()
                    && !has_key("ANTHROPIC_API_KEY")
                    && !has_key("OPENAI_API_KEY")
                    && !has_key("GOOGLE_API_KEY")
            }
            _ => false,
        };

        if missing {
            println!();
            println!(
                "⚠  Agent '{}' has no credentials. Set an API key:",
                agent.name
            );
            println!("     hive auth set-key ANTHROPIC_API_KEY sk-ant-...");
        }
    }
}

/// `hive auth status` — show what auth credentials are detected for each agent.
pub fn auth_status(project_dir: &Path) -> Result<()> {
    let cfg = load_config(project_dir)?;
    let hive = hive_dir(project_dir);
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/root"));

    // Read .hive/.env keys (show masked values).
    let dotenv_path = hive.join(".env");
    let dotenv_keys: Vec<String> = if dotenv_path.exists() {
        std::fs::read_to_string(&dotenv_path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
            .filter_map(|l| {
                let (k, v) = l.split_once('=')?;
                let k = k.trim();
                let v = v.trim();
                let masked = if v.len() > 8 {
                    format!("{}***", &v[..8])
                } else {
                    "***".to_string()
                };
                Some(format!("  {k}={masked}"))
            })
            .collect()
    } else {
        vec![]
    };

    let claude_json_host = home.join(".claude.json");
    let claude_json_hive = hive.join("claude.json");
    let claude_dir_host = home.join(".claude");
    let kilocode_dir_host = home.join(".kilocode");

    println!("Auth status for {}", hive.display());
    println!();

    // .hive/.env keys
    if dotenv_keys.is_empty() {
        println!("  .hive/.env         — not found (no API keys configured)");
    } else {
        println!("  .hive/.env         — found:");
        for line in &dotenv_keys {
            println!("{line}");
        }
    }
    println!();

    // Per-agent summary
    for agent in &cfg.agents {
        println!("Agent '{}' ({})", agent.name, agent.coding_agent);
        match agent.coding_agent.as_str() {
            "claude" => {
                let host_ok = check("~/.claude.json (host login)", claude_json_host.exists());
                let hive_ok = check(
                    ".hive/claude.json (synced creds)",
                    claude_json_hive.exists(),
                );
                let dir_ok = check("~/.claude/ (settings dir)", claude_dir_host.exists());
                let creds_host_ok = check(
                    "~/.claude/.credentials.json (OAuth creds)",
                    claude_dir_host.join(".credentials.json").exists(),
                );
                let creds_hive_ok = check(
                    ".hive/claude-credentials.json (synced OAuth creds)",
                    hive.join("claude-credentials.json").exists(),
                );
                let key_ok = dotenv_keys.iter().any(|l| l.contains("ANTHROPIC_API_KEY"));
                let _key_msg = check(".hive/.env ANTHROPIC_API_KEY", key_ok);

                if !host_ok && !hive_ok && !key_ok {
                    println!("  ⚠  No claude credentials found. Options:");
                    println!("       API key:      hive auth set-key ANTHROPIC_API_KEY sk-ant-...");
                    println!("       Subscription: hive auth sync  (copies ~/.claude.json)");
                    println!("                  or hive auth login (login inside container)");
                }
                let _ = (dir_ok, creds_host_ok, creds_hive_ok);
            }
            "kilo" => {
                let dir_ok = check("~/.kilocode/ (kilo settings)", kilocode_dir_host.exists());
                let key_ok = dotenv_keys.iter().any(|l| {
                    l.contains("ANTHROPIC_API_KEY")
                        || l.contains("OPENAI_API_KEY")
                        || l.contains("GOOGLE_API_KEY")
                });
                let _key_msg = check(".hive/.env API key (ANTHROPIC/OPENAI/GOOGLE)", key_ok);

                if !key_ok {
                    println!("  ⚠  No API key found for kilo. Set one with:");
                    println!("       hive auth set-key ANTHROPIC_API_KEY sk-ant-...");
                    println!("       hive auth set-key OPENAI_API_KEY sk-...");
                }
                let _ = dir_ok;
            }
            other => {
                println!("  (unknown agent type '{other}')");
            }
        }
        println!();
    }

    Ok(())
}

fn check(label: &str, present: bool) -> bool {
    let icon = if present { "✓" } else { "✗" };
    println!("  {icon}  {label}");
    present
}

/// `hive auth sync` — copy ~/.claude.json and ~/.claude/.credentials.json to .hive/ for use in agent containers.
pub fn auth_sync(project_dir: &Path) -> Result<()> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    let hive = hive_dir(project_dir);

    let config_src = home.join(".claude.json");
    let creds_src = home.join(".claude/.credentials.json");

    if !config_src.exists() && !creds_src.exists() {
        anyhow::bail!(
            "Neither ~/.claude.json nor ~/.claude/.credentials.json found.\n\
             Run 'claude auth login' on the host first, or use 'hive auth login' to authenticate inside a container."
        );
    }

    if config_src.exists() {
        let dst = hive.join("claude.json");
        std::fs::copy(&config_src, &dst).context("copying ~/.claude.json to .hive/claude.json")?;
        println!("Copied ~/.claude.json → .hive/claude.json");
    }

    if creds_src.exists() {
        let dst = hive.join("claude-credentials.json");
        std::fs::copy(&creds_src, &dst)
            .context("copying ~/.claude/.credentials.json to .hive/claude-credentials.json")?;
        println!("Copied ~/.claude/.credentials.json → .hive/claude-credentials.json");
    } else {
        println!(
            "Note: ~/.claude/.credentials.json not found (OAuth credentials may not be set up)."
        );
    }

    println!("Files will be auto-mounted in claude agent containers.");
    println!("Run 'hive restart' to apply to running containers.");
    Ok(())
}

/// `hive auth kilo-sync --agent NAME` — select a kilo provider and write a minimal
/// per-agent kilocode config to `.hive/kilocode-{name}/cli/config.json`.
///
/// Reads `~/.kilocode/cli/config.json`, lists the available providers, prompts the
/// user to pick one, then writes a minimal config with just that provider (renamed to
/// `"default"`) to the per-agent directory.
pub fn auth_kilo_sync(project_dir: &Path, agent: Option<&str>) -> Result<()> {
    let src_config = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join(".kilocode/cli/config.json");

    if !src_config.exists() {
        anyhow::bail!(
            "~/.kilocode/cli/config.json not found.\n\
             Install Kilo and run it at least once to create the config."
        );
    }

    let raw =
        std::fs::read_to_string(&src_config).context("reading ~/.kilocode/cli/config.json")?;
    let json: serde_json::Value =
        serde_json::from_str(&raw).context("parsing ~/.kilocode/cli/config.json")?;

    let providers: Vec<(usize, String)> = json["providers"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .enumerate()
                .filter_map(|(i, p)| p["id"].as_str().map(|id| (i, id.to_string())))
                .collect()
        })
        .unwrap_or_default();

    if providers.is_empty() {
        anyhow::bail!("No providers found in ~/.kilocode/cli/config.json");
    }

    // Print provider list and prompt.
    println!("Available kilo providers:");
    for (i, (_, id)) in providers.iter().enumerate() {
        println!("  [{i}] {id}");
    }

    let selected_idx = if providers.len() == 1 {
        println!("Only one provider found — using '{}'.", providers[0].1);
        0
    } else {
        print!("Select provider [0-{}]: ", providers.len() - 1);
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        line.trim().parse::<usize>().context("invalid selection")?
    };

    let (arr_idx, provider_id) = providers
        .get(selected_idx)
        .ok_or_else(|| anyhow::anyhow!("selection out of range"))?;

    // Build the minimal provider object with id renamed to "default".
    let mut provider = json["providers"][*arr_idx].clone();
    if let Some(obj) = provider.as_object_mut() {
        obj.insert(
            "id".to_string(),
            serde_json::Value::String("default".to_string()),
        );
    }

    let out = serde_json::json!({
        "providers": [provider],
        "provider": "default"
    });

    let dst_name = match agent {
        Some(name) => format!("kilocode-{name}"),
        None => "kilocode".to_string(),
    };
    let dst_dir = hive_dir(project_dir).join(&dst_name).join("cli");
    std::fs::create_dir_all(&dst_dir).with_context(|| format!("creating .hive/{dst_name}/cli/"))?;
    std::fs::write(
        dst_dir.join("config.json"),
        serde_json::to_string_pretty(&out)?,
    )
    .with_context(|| format!("writing .hive/{dst_name}/cli/config.json"))?;

    println!("Written provider '{provider_id}' → .hive/{dst_name}/cli/config.json");
    if let Some(name) = agent {
        println!("Per-agent config will be mounted for agent '{name}'.");
    } else {
        println!("Shared config will be mounted as /home/agent/.kilocode in all kilo containers.");
    }
    println!("Run 'hive restart' to apply to running containers.");
    Ok(())
}

/// `hive auth login [--email]` — run `claude auth login` inside the first agent container,
/// stream the URL to stdout, and copy the resulting credentials to .hive/claude.json.
pub async fn auth_login(project_dir: &Path, email: Option<&str>) -> Result<()> {
    let cfg = load_config(project_dir)?;
    let id = &cfg.project_id;
    let agent = cfg
        .agents
        .first()
        .ok_or_else(|| anyhow::anyhow!("No agents configured in .hive/config.toml"))?;
    let container = containers::agent_name(id, &agent.name);

    println!("Running 'claude auth login' in container '{container}'…");

    let mut cmd = std::process::Command::new("docker");
    cmd.arg("exec")
        .arg("-i")
        .arg(&container)
        .arg("claude")
        .arg("auth")
        .arg("login");
    if let Some(email) = email {
        cmd.arg("--email").arg(email);
    }

    let status = cmd.status().context("running docker exec")?;
    if !status.success() {
        anyhow::bail!("claude auth login exited with status {status}");
    }

    // Copy credentials from the container back to .hive/.
    let hive = hive_dir(project_dir);

    let dst = hive.join("claude.json");
    let src_in_container = format!("{container}:/home/agent/.claude.json");
    let cp_status = std::process::Command::new("docker")
        .args(["cp", &src_in_container, dst.to_str().unwrap_or(".")])
        .status()
        .context("copying .claude.json from container")?;

    if cp_status.success() {
        println!("Copied .claude.json → .hive/claude.json");
    } else {
        println!("Warning: could not copy .claude.json from container.");
    }

    // Also copy OAuth credentials if they were created.
    let creds_dst = hive.join("claude-credentials.json");
    let creds_src = format!("{container}:/home/agent/.claude/.credentials.json");
    let creds_status = std::process::Command::new("docker")
        .args(["cp", &creds_src, creds_dst.to_str().unwrap_or(".")])
        .status()
        .context("copying .credentials.json from container")?;

    if creds_status.success() {
        println!("Copied .credentials.json → .hive/claude-credentials.json");
    }

    if cp_status.success() || creds_status.success() {
        println!("Run 'hive restart' to mount the new credentials into all agent containers.");
    } else {
        println!("Warning: could not copy credentials from container. Try 'hive auth sync' after authenticating on the host.");
    }

    Ok(())
}

/// `hive logs [container] [-f]` — stream logs from one or all containers.
///
/// `container` can be:
/// - `"all"` (default): interleave logs from all project containers with `[name]` prefixes
/// - `"server"` / `"app"` / agent name: resolve to the project-scoped container name
/// - full container name: used as-is
pub async fn logs(project_dir: &Path, container: &str, follow: bool) -> Result<()> {
    use bollard::query_parameters::LogsOptionsBuilder;
    use futures_util::StreamExt;

    let cfg = load_config(project_dir)?;
    let docker = connect_docker()?;
    let id = &cfg.project_id;

    // Build list of (alias, full_container_name) pairs for this project.
    let mut all_targets: Vec<(String, String)> = vec![
        ("server".to_string(), containers::server_name(id)),
        ("app".to_string(), containers::app_name(id)),
    ];
    for agent in &cfg.agents {
        all_targets.push((agent.name.clone(), containers::agent_name(id, &agent.name)));
    }

    let selected: Vec<(String, String)> = if container == "all" {
        all_targets
    } else {
        // Resolve short alias to full name, or use as-is for explicit container names.
        let full = all_targets
            .iter()
            .find(|(alias, _)| alias == container)
            .map_or_else(|| container.to_string(), |(_, full)| full.clone());
        vec![(container.to_string(), full)]
    };

    let tail = if follow { "all" } else { "100" };

    if selected.len() == 1 {
        let (_, name) = &selected[0];
        let opts = LogsOptionsBuilder::default()
            .stdout(true)
            .stderr(true)
            .follow(follow)
            .tail(tail)
            .build();
        let mut stream = docker.logs(name, Some(opts));
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(output) => print!("{output}"),
                Err(e) => eprintln!("log error: {e}"),
            }
        }
    } else {
        // Stream all containers in parallel; each task prints with a [alias] prefix.
        let prefix_width = selected.iter().map(|(a, _)| a.len()).max().unwrap_or(6);
        let handles: Vec<_> = selected
            .into_iter()
            .map(|(alias, name)| {
                let docker = docker.clone();
                tokio::spawn(async move {
                    let opts = LogsOptionsBuilder::default()
                        .stdout(true)
                        .stderr(true)
                        .follow(follow)
                        .tail(tail)
                        .build();
                    let mut stream = docker.logs(&name, Some(opts));
                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(output) => {
                                print!("[{alias:<width$}] {output}", width = prefix_width)
                            }
                            Err(e) => eprintln!("[{alias}] log error: {e}"),
                        }
                    }
                })
            })
            .collect();
        for h in handles {
            let _ = h.await;
        }
    }
    Ok(())
}
