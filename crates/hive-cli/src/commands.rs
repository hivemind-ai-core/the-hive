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
    Docker::connect_with_local_defaults().context("connecting to Docker")
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

    // Ensure network exists.
    network::ensure(&docker, id).await?;

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

/// `hive logs <container>` — stream logs from a container.
pub async fn logs(project_dir: &Path, container: &str) -> Result<()> {
    use bollard::query_parameters::LogsOptionsBuilder;
    use futures_util::StreamExt;

    let _ = load_config(project_dir)?; // validate config exists
    let docker = connect_docker()?;

    let opts = LogsOptionsBuilder::default()
        .stdout(true)
        .stderr(true)
        .follow(false)
        .tail("100")
        .build();

    let mut stream = docker.logs(container, Some(opts));
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(output) => print!("{output}"),
            Err(e) => eprintln!("log error: {e}"),
        }
    }
    Ok(())
}
