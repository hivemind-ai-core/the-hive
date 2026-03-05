//! Container lifecycle operations: start, stop, restart, remove, health wait.

use std::time::Duration;

use anyhow::{Context, Result};
use bollard::{
    Docker,
    models::HealthStatusEnum,
    query_parameters::{
        RemoveContainerOptionsBuilder, StartContainerOptionsBuilder,
        StopContainerOptionsBuilder,
    },
};
use tracing::{info, warn};

const HIVE_CONTAINERS: &[&str] = &["hive-server", "hive-app"];

/// All container names for the current config (server + app + agents).
pub fn all_container_names(agent_count: u8) -> Vec<String> {
    let mut names: Vec<String> = HIVE_CONTAINERS.iter().map(|s| s.to_string()).collect();
    for i in 1..=agent_count {
        names.push(format!("hive-agent-{i}"));
    }
    names
}

pub async fn start(docker: &Docker, name: &str) -> Result<()> {
    let opts = StartContainerOptionsBuilder::default().build();
    docker
        .start_container(name, Some(opts))
        .await
        .with_context(|| format!("starting container '{name}'"))?;
    info!("Started '{name}'");
    Ok(())
}

pub async fn stop(docker: &Docker, name: &str) -> Result<()> {
    let opts = StopContainerOptionsBuilder::default().t(10).build();
    match docker.stop_container(name, Some(opts)).await {
        Ok(_) => info!("Stopped '{name}'"),
        Err(e) if e.to_string().contains("not running")
               || e.to_string().contains("not found")
               || e.to_string().contains("No such") => {}
        Err(e) => return Err(e).context(format!("stopping container '{name}'")),
    }
    Ok(())
}

pub async fn restart(docker: &Docker, name: &str) -> Result<()> {
    stop(docker, name).await?;
    start(docker, name).await
}

pub async fn remove(docker: &Docker, name: &str) -> Result<()> {
    let opts = RemoveContainerOptionsBuilder::default()
        .force(true)
        .build();
    match docker.remove_container(name, Some(opts)).await {
        Ok(_) => info!("Removed '{name}'"),
        Err(e) if e.to_string().contains("not found") || e.to_string().contains("No such") => {}
        Err(e) => return Err(e).context(format!("removing container '{name}'")),
    }
    Ok(())
}

/// Poll container health until it reaches "healthy" or timeout expires.
pub async fn wait_healthy(docker: &Docker, name: &str, timeout: Duration) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let info = docker
            .inspect_container(name, None)
            .await
            .with_context(|| format!("inspecting '{name}'"))?;

        let state = info.state.as_ref();
        let health = state
            .and_then(|s| s.health.as_ref())
            .and_then(|h| h.status.as_ref());

        match health {
            Some(HealthStatusEnum::HEALTHY) => {
                info!("Container '{name}' is healthy");
                return Ok(());
            }
            Some(HealthStatusEnum::UNHEALTHY) => anyhow::bail!("container '{name}' is unhealthy"),
            _ => {
                // No healthcheck or starting — check if running at least.
                if state.and_then(|s| s.running).unwrap_or(false) {
                    info!("Container '{name}' is running (no healthcheck)");
                    return Ok(());
                }
            }
        }

        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("timeout waiting for '{name}' to become healthy");
        }
        warn!("Waiting for '{name}'...");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
