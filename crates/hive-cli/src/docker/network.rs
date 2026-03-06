//! Docker network management.

use std::collections::HashMap;

use anyhow::{Context, Result};
use bollard::{
    Docker,
    models::NetworkCreateRequest,
    query_parameters::ListNetworksOptionsBuilder,
};
use tracing::info;

/// Return the project-scoped external network name (server + app-daemon, host-accessible).
pub fn network_name(project_id: &str) -> String {
    format!("hive-net-{project_id}")
}

/// Return the project-scoped agent network name (agents + server, optionally internal).
pub fn agent_network_name(project_id: &str) -> String {
    format!("hive-agents-{project_id}")
}

/// Ensure the external project network exists; create it if not.
pub async fn ensure(docker: &Docker, project_id: &str) -> Result<()> {
    let name = network_name(project_id);
    if exists(docker, &name).await? {
        info!("Network '{name}' already exists");
        return Ok(());
    }
    create(docker, &name, false).await
}

/// Ensure the agent network exists; create it if not.
///
/// If `internal` is true the network is created without an external route
/// (agents cannot reach the internet). Existing networks are not modified.
pub async fn ensure_agent_network(docker: &Docker, project_id: &str, internal: bool) -> Result<()> {
    let name = agent_network_name(project_id);
    if exists(docker, &name).await? {
        info!("Network '{name}' already exists");
        return Ok(());
    }
    create(docker, &name, internal).await
}

/// Return true if the named network exists.
pub async fn exists(docker: &Docker, name: &str) -> Result<bool> {
    let mut filters = HashMap::new();
    filters.insert("name".to_string(), vec![name.to_string()]);
    let opts = ListNetworksOptionsBuilder::default()
        .filters(&filters)
        .build();
    let networks = docker
        .list_networks(Some(opts))
        .await
        .context("listing Docker networks")?;
    Ok(networks.iter().any(|n| n.name.as_deref() == Some(name)))
}

/// Create the named network as a bridge.
///
/// When `internal` is true, the network is created with the Docker `internal` flag,
/// which prevents containers from reaching the internet while still allowing
/// container-to-container communication within the network.
pub async fn create(docker: &Docker, name: &str, internal: bool) -> Result<()> {
    let req = NetworkCreateRequest {
        name: name.to_string(),
        driver: Some("bridge".to_string()),
        internal: Some(internal),
        ..Default::default()
    };
    docker
        .create_network(req)
        .await
        .context("creating Docker network")?;
    if internal {
        info!("Created internal network '{name}' (no internet access for agents)");
    } else {
        info!("Created network '{name}'");
    }
    Ok(())
}

/// Remove both project networks (best-effort; ignores not-found).
pub async fn remove_all(docker: &Docker, project_id: &str) -> Result<()> {
    remove(docker, &network_name(project_id)).await?;
    remove(docker, &agent_network_name(project_id)).await
}

/// Remove the named network (best-effort; ignores not-found).
pub async fn remove(docker: &Docker, name: &str) -> Result<()> {
    match docker.remove_network(name).await {
        Ok(_) => {
            info!("Removed network '{name}'");
            Ok(())
        }
        Err(e) if e.to_string().contains("not found") => Ok(()),
        Err(e) => Err(e).context("removing Docker network"),
    }
}
