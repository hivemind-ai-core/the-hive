//! Docker network management.

use std::collections::HashMap;

use anyhow::{Context, Result};
use bollard::{
    Docker,
    models::NetworkCreateRequest,
    query_parameters::ListNetworksOptionsBuilder,
};
use tracing::info;

/// Return the project-scoped network name.
pub fn network_name(project_id: &str) -> String {
    format!("hive-net-{project_id}")
}

/// Ensure the project network exists; create it if not.
pub async fn ensure(docker: &Docker, project_id: &str) -> Result<()> {
    let name = network_name(project_id);
    if exists(docker, &name).await? {
        info!("Network '{name}' already exists");
        return Ok(());
    }
    create(docker, &name).await
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
pub async fn create(docker: &Docker, name: &str) -> Result<()> {
    let req = NetworkCreateRequest {
        name: name.to_string(),
        driver: Some("bridge".to_string()),
        ..Default::default()
    };
    docker
        .create_network(req)
        .await
        .context("creating Docker network")?;
    info!("Created network '{name}'");
    Ok(())
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
