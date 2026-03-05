//! CLI command implementations.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use bollard::Docker;
use tokio::process::Command;
use tracing::info;

use crate::config::{self, Config};
use crate::config::io::{hive_dir, default_path};
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
        (containers::agent_image(id),  "Dockerfile.agent",  "agent"),
        (containers::app_image(id),    "Dockerfile.app",    "app"),
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

    // Ensure network exists (internal = no internet for agents if isolation enabled).
    network::ensure(&docker, id, cfg.network.isolate).await?;

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
    if let Err(e) = containers::create_agents(&docker, &cfg, project_dir).await {
        if !format!("{e:#}").contains("already in use") {
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
    Ok(())
}

/// `hive stop [--remove]` — stop all containers, optionally removing them.
pub async fn stop(project_dir: &Path, remove: bool) -> Result<()> {
    let cfg = load_config(project_dir)?;
    let docker = connect_docker()?;
    let id = &cfg.project_id;

    for agent in cfg.agents.iter().rev() {
        lifecycle::stop(&docker, &containers::agent_name(id, &agent.name)).await?;
    }
    lifecycle::stop(&docker, &containers::app_name(id)).await?;
    lifecycle::stop(&docker, &containers::server_name(id)).await?;

    if remove {
        for agent in cfg.agents.iter().rev() {
            lifecycle::remove(&docker, &containers::agent_name(id, &agent.name)).await?;
        }
        lifecycle::remove(&docker, &containers::app_name(id)).await?;
        lifecycle::remove(&docker, &containers::server_name(id)).await?;
        network::remove(&docker, &network::network_name(id)).await?;
        info!("All containers stopped and removed");
    } else {
        info!("All containers stopped");
    }
    Ok(())
}

/// `hive restart` — restart all containers.
pub async fn restart(project_dir: &Path) -> Result<()> {
    stop(project_dir, false).await?;
    start(project_dir).await
}

/// `hive rebuild [target]` — rebuild Docker images and replace running containers.
pub async fn rebuild(project_dir: &Path, target: &str) -> Result<()> {
    let cfg = load_config(project_dir)?;
    let id = &cfg.project_id;
    let hive = hive_dir(project_dir);
    let docker = connect_docker()?;

    let all = [
        ("server", "Dockerfile.server", containers::server_image(id), containers::server_name(id)),
        ("agent",  "Dockerfile.agent",  containers::agent_image(id),  String::new()), // image-only target
        ("app",    "Dockerfile.app",    containers::app_image(id),    containers::app_name(id)),
    ];

    let builds: Vec<_> = if target == "all" {
        all.iter().collect()
    } else {
        all.iter().filter(|(name, _, _, _)| *name == target).collect()
    };

    anyhow::ensure!(!builds.is_empty(), "Unknown target '{target}'. Use: server, agent, app, all");

    // Build new images.
    for (name, dockerfile, tag, _) in &builds {
        let dockerfile_path = hive.join(dockerfile);
        anyhow::ensure!(dockerfile_path.exists(), "Dockerfile not found: {}", dockerfile_path.display());

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
            "app"    => lifecycle::remove(&docker, &containers::app_name(id)).await?,
            "agent"  => {
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

    println!("{:<35} {:<15} {}", "CONTAINER", "STATUS", "ID");
    println!("{}", "-".repeat(65));

    for name in &names {
        match docker.inspect_container(name, None).await {
            Ok(info) => {
                let status = info
                    .state
                    .as_ref()
                    .and_then(|s| s.status.as_ref())
                    .map(|s| format!("{s:?}"))
                    .unwrap_or_else(|| "unknown".to_string());
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

/// `hive auth set-key KEY VALUE` — write an API key to `.hive/.env`.
pub fn auth_set_key(project_dir: &Path, key: &str, value: &str) -> Result<()> {
    anyhow::ensure!(!key.is_empty(), "key must not be empty");
    anyhow::ensure!(!value.is_empty(), "value must not be empty");

    let env_path = hive_dir(project_dir).join(".env");
    dotenv_set(&env_path, key, value)?;

    let masked = if value.len() > 8 { format!("{}***", &value[..8]) } else { "***".to_string() };
    println!("Set {key}={masked} in .hive/.env");
    println!("Run 'hive restart' to apply to running containers.");
    Ok(())
}

/// `hive auth set-endpoint KEY URL` — write a base URL to `.hive/.env`.
pub fn auth_set_endpoint(project_dir: &Path, key: &str, url: &str) -> Result<()> {
    anyhow::ensure!(!key.is_empty(), "key must not be empty");
    anyhow::ensure!(!url.is_empty(), "url must not be empty");

    let env_path = hive_dir(project_dir).join(".env");
    dotenv_set(&env_path, key, url)?;

    println!("Set {key}={url} in .hive/.env");
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
                let host_ok  = check("~/.claude.json (host login)", claude_json_host.exists());
                let hive_ok  = check(".hive/claude.json (synced creds)", claude_json_hive.exists());
                let dir_ok   = check("~/.claude/ (settings dir)", claude_dir_host.exists());
                let key_ok   = dotenv_keys.iter().any(|l| l.contains("ANTHROPIC_API_KEY"));
                let _key_msg = check(".hive/.env ANTHROPIC_API_KEY", key_ok);

                if !host_ok && !hive_ok && !key_ok {
                    println!("  ⚠  No claude credentials found. Options:");
                    println!("       API key:      hive auth set-key ANTHROPIC_API_KEY sk-ant-...");
                    println!("       Subscription: hive auth sync  (copies ~/.claude.json)");
                    println!("                  or hive auth login (login inside container)");
                }
                let _ = dir_ok;
            }
            "kilo" => {
                let dir_ok  = check("~/.kilocode/ (kilo settings)", kilocode_dir_host.exists());
                let key_ok  = dotenv_keys.iter().any(|l| {
                    l.contains("ANTHROPIC_API_KEY") || l.contains("OPENAI_API_KEY")
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

/// `hive auth sync` — copy ~/.claude.json to .hive/claude.json for use in agent containers.
pub fn auth_sync(project_dir: &Path) -> Result<()> {
    let src = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join(".claude.json");

    if !src.exists() {
        anyhow::bail!(
            "~/.claude.json not found.\n\
             Run 'claude auth login' on the host first, or use 'hive auth login' to authenticate inside a container."
        );
    }

    let dst = hive_dir(project_dir).join("claude.json");
    std::fs::copy(&src, &dst).context("copying ~/.claude.json to .hive/claude.json")?;
    println!("Copied ~/.claude.json → .hive/claude.json");
    println!("The credentials will be auto-mounted as /home/agent/.claude.json in claude agent containers.");
    println!("Run 'hive restart' to apply to running containers.");
    Ok(())
}

/// `hive auth kilo-sync` — copy ~/.kilocode/ to .hive/kilocode/ for project-local kilo config.
///
/// Once synced, the project-local copy is mounted instead of the global one,
/// allowing per-project kilo settings without affecting other projects.
pub fn auth_kilo_sync(project_dir: &Path) -> Result<()> {
    let src = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join(".kilocode");

    if !src.exists() {
        anyhow::bail!(
            "~/.kilocode not found.\n\
             Install Kilo and run it at least once to create the config directory."
        );
    }

    let dst = hive_dir(project_dir).join("kilocode");
    if dst.exists() {
        std::fs::remove_dir_all(&dst).context("removing existing .hive/kilocode/")?;
    }
    copy_dir_all(&src, &dst).context("copying ~/.kilocode to .hive/kilocode/")?;
    println!("Copied ~/.kilocode → .hive/kilocode/");
    println!("The directory will be auto-mounted as /home/agent/.kilocode in kilo agent containers.");
    println!("Run 'hive restart' to apply to running containers.");
    Ok(())
}

/// Recursively copy a directory.
fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

/// `hive auth login [--email]` — run `claude auth login` inside the first agent container,
/// stream the URL to stdout, and copy the resulting credentials to .hive/claude.json.
pub async fn auth_login(project_dir: &Path, email: Option<&str>) -> Result<()> {
    let cfg = load_config(project_dir)?;
    let id = &cfg.project_id;
    let agent = cfg.agents.first()
        .ok_or_else(|| anyhow::anyhow!("No agents configured in .hive/config.toml"))?;
    let container = containers::agent_name(id, &agent.name);

    println!("Running 'claude auth login' in container '{container}'…");

    let mut cmd = std::process::Command::new("docker");
    cmd.arg("exec").arg("-i").arg(&container).arg("claude").arg("auth").arg("login");
    if let Some(email) = email {
        cmd.arg("--email").arg(email);
    }

    let status = cmd.status().context("running docker exec")?;
    if !status.success() {
        anyhow::bail!("claude auth login exited with status {status}");
    }

    // Copy credentials from the container back to .hive/claude.json.
    let dst = hive_dir(project_dir).join("claude.json");
    let src_in_container = format!("{container}:/home/agent/.claude.json");
    let cp_status = std::process::Command::new("docker")
        .args(["cp", &src_in_container, dst.to_str().unwrap_or(".")])
        .status()
        .context("copying .claude.json from container")?;

    if cp_status.success() {
        println!("Credentials saved to .hive/claude.json");
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
            .map(|(_, full)| full.clone())
            .unwrap_or_else(|| container.to_string());
        vec![(container.to_string(), full)]
    };

    if selected.len() == 1 {
        let (_, name) = &selected[0];
        let opts = LogsOptionsBuilder::default()
            .stdout(true)
            .stderr(true)
            .follow(follow)
            .tail("100")
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
                        .tail("100")
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
