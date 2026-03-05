//! Container creation helpers.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use bollard::{
    Docker,
    models::{ContainerCreateBody, HostConfig, PortBinding},
};
use tracing::info;

use super::network::network_name;
use crate::config::{Agent, Config};

/// Image name for hive-server, scoped to the project.
pub fn server_image(id: &str) -> String { format!("hive-server-{id}:latest") }
/// Image name for hive-agent, scoped to the project.
pub fn agent_image(id: &str) -> String  { format!("hive-agent-{id}:latest") }
/// Image name for app-container, scoped to the project.
pub fn app_image(id: &str) -> String    { format!("app-container-{id}:latest") }

/// Container name for hive-server.
pub fn server_name(id: &str) -> String  { format!("hive-server-{id}") }
/// Container name for app.
pub fn app_name(id: &str) -> String     { format!("hive-app-{id}") }
/// Container name for a named agent.
pub fn agent_name(id: &str, agent: &str) -> String {
    let safe: String = agent
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
        .collect();
    format!("hive-agent-{id}-{safe}")
}

/// Check whether a Docker image exists locally.
pub async fn image_exists(docker: &Docker, image: &str) -> Result<bool> {
    match docker.inspect_image(image).await {
        Ok(_) => Ok(true),
        Err(bollard::errors::Error::DockerResponseServerError { status_code: 404, .. }) => Ok(false),
        Err(e) => Err(e).context("checking image existence"),
    }
}

/// Create the hive-server container.
pub async fn create_server(docker: &Docker, cfg: &Config, project_dir: &Path) -> Result<String> {
    let project_dir = project_dir.canonicalize().context("resolving project directory")?;
    let id = &cfg.project_id;
    let name = server_name(id);
    let net = network_name(id);
    let hive_dir = project_dir.join(".hive");
    let container_port = format!("{}/tcp", cfg.server.port);
    let host_port = cfg.server.host_port.to_string();

    let mut port_bindings = HashMap::new();
    port_bindings.insert(
        container_port,
        Some(vec![PortBinding {
            host_ip: Some("0.0.0.0".to_string()),
            host_port: Some(host_port),
        }]),
    );

    let body = ContainerCreateBody {
        image: Some(server_image(id)),
        env: Some(vec![
            format!("HIVE_SERVER_PORT={}", cfg.server.port),
            format!("HIVE_DB_PATH={}", cfg.server.db_path),
            format!("RUST_LOG={}", cfg.logging.level),
        ]),
        host_config: Some(HostConfig {
            port_bindings: Some(port_bindings),
            network_mode: Some(net),
            // Mount .hive/ as /data (server stores its DB there)
            binds: Some(vec![
                format!("{}:/data", hive_dir.display()),
            ]),
            ..Default::default()
        }),
        ..Default::default()
    };

    let container_id = docker
        .create_container(
            Some(bollard::query_parameters::CreateContainerOptionsBuilder::default()
                .name(&name)
                .build()),
            body,
        )
        .await
        .context("creating hive-server container")?
        .id;

    info!("Created container '{name}' ({container_id})");
    Ok(container_id)
}

/// Create the app-container.
pub async fn create_app(docker: &Docker, cfg: &Config, project_dir: &Path) -> Result<String> {
    let project_dir = project_dir.canonicalize().context("resolving project directory")?;
    let id = &cfg.project_id;
    let name = app_name(id);
    let net = network_name(id);
    let hive_dir = project_dir.join(".hive");
    let project_dir_str = project_dir.display().to_string();
    let daemon_container_port = format!("{}/tcp", cfg.app.daemon_port);
    let daemon_host_port = cfg.app.daemon_host_port.to_string();

    let mut port_bindings = HashMap::new();
    port_bindings.insert(
        daemon_container_port,
        Some(vec![PortBinding {
            host_ip: Some("0.0.0.0".to_string()),
            host_port: Some(daemon_host_port),
        }]),
    );

    let body = ContainerCreateBody {
        image: Some(app_image(id)),
        env: Some(vec![
            format!("HIVE_APP_DAEMON_PORT={}", cfg.app.daemon_port),
            format!("RUST_LOG={}", cfg.logging.level),
        ]),
        host_config: Some(HostConfig {
            port_bindings: Some(port_bindings),
            network_mode: Some(net),
            binds: Some(vec![
                format!("{project_dir_str}:/app"),
                format!("{}:/app/.hive:ro", hive_dir.display()),
            ]),
            ..Default::default()
        }),
        ..Default::default()
    };

    let container_id = docker
        .create_container(
            Some(bollard::query_parameters::CreateContainerOptionsBuilder::default()
                .name(&name)
                .build()),
            body,
        )
        .await
        .context("creating app container")?
        .id;

    info!("Created container '{name}' ({container_id})");
    Ok(container_id)
}

/// Load a `.env`-format file into a `HashMap`. Lines starting with `#` and
/// blank lines are ignored. Each valid line must be `KEY=value`.
fn load_dotenv(path: &Path) -> HashMap<String, String> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    contents
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Create one hive-agent container per entry in `cfg.agents`.
pub async fn create_agents(docker: &Docker, cfg: &Config, project_dir: &Path) -> Result<Vec<String>> {
    let project_dir = project_dir.canonicalize().context("resolving project directory")?;
    let id = &cfg.project_id;
    let net = network_name(id);
    let hive_dir = project_dir.join(".hive");
    let project_dir_str = project_dir.display().to_string();
    let server_url = format!("ws://{}:{}/ws", server_name(id), cfg.server.port);
    let app_daemon_url = format!("http://{}:{}", app_name(id), cfg.app.daemon_port);

    // Load project-wide .hive/.env (missing file → empty map, silently).
    let dotenv = load_dotenv(&hive_dir.join(".env"));

    let mut ids = Vec::new();

    for (idx, agent) in cfg.agents.iter().enumerate() {
        let name = agent_name(id, &agent.name);
        let mcp_port = 7890u16 + idx as u16;

        // Merge: dotenv < agent.env (agent-specific overrides project-wide).
        let mut merged_env = dotenv.clone();
        merged_env.extend(agent.env.clone());

        let container_id = create_agent_container(
            docker, id, &name, agent, &net, &server_url, &app_daemon_url,
            &project_dir_str, &hive_dir.display().to_string(), mcp_port,
            &cfg.logging.level, &merged_env,
        ).await?;

        info!("Created container '{name}' ({container_id})");
        ids.push(container_id);
    }

    Ok(ids)
}

async fn create_agent_container(
    docker: &Docker,
    id: &str,
    name: &str,
    agent: &Agent,
    net: &str,
    server_url: &str,
    app_daemon_url: &str,
    project_dir: &str,
    hive_dir: &str,
    mcp_port: u16,
    log_level: &str,
    extra_env: &HashMap<String, String>,
) -> Result<String> {
    let mut binds = vec![
        format!("{project_dir}:/app"),
        format!("{hive_dir}:/app/.hive:ro"),
    ];

    // Auto-mount credential directories for known coding agents.
    // Only mounted if the source directory exists on the host.
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let cred_mounts: &[(&str, &str)] = match agent.coding_agent.as_str() {
        "claude" => &[(".claude", "/home/agent/.claude")],
        "kilo"   => &[(".kilocode", "/home/agent/.kilocode")],
        _        => &[],
    };
    for (src_name, container_path) in cred_mounts {
        let src = std::path::Path::new(&home).join(src_name);
        if src.exists() {
            binds.push(format!("{}:{container_path}", src.display()));
        }
    }

    // Build env: fixed hive vars first, then caller-supplied extras.
    let mut env: Vec<String> = vec![
        format!("HIVE_AGENT_ID={}", agent.name),
        format!("HIVE_AGENT_NAME={}", agent.name),
        format!("HIVE_AGENT_TAGS={}", agent.tags.join(",")),
        format!("HIVE_SERVER_URL={server_url}"),
        format!("HIVE_APP_DAEMON_URL={app_daemon_url}"),
        format!("CODING_AGENT={}", agent.coding_agent),
        format!("HIVE_MCP_PORT={mcp_port}"),
        format!("RUST_LOG={log_level}"),
    ];
    for (k, v) in extra_env {
        env.push(format!("{k}={v}"));
    }

    let body = ContainerCreateBody {
        image: Some(agent_image(id)),
        env: Some(env),
        host_config: Some(HostConfig {
            network_mode: Some(net.to_string()),
            binds: Some(binds),
            ..Default::default()
        }),
        ..Default::default()
    };

    docker
        .create_container(
            Some(bollard::query_parameters::CreateContainerOptionsBuilder::default()
                .name(name)
                .build()),
            body,
        )
        .await
        .with_context(|| format!("creating agent container '{name}'"))
        .map(|r| r.id)
}
