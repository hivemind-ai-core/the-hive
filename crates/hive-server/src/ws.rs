//! HTTP + WebSocket server: /health for Docker healthcheck, /ws for agents.

use anyhow::Result;
use axum::{
    Router,
    extract::{Query, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::IntoResponse,
    routing::get,
};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use hive_core::types::{ApiError, ApiMessage, MessageType};
use std::collections::HashMap;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    agent_registry::{self, AgentState},
    communication,
    handlers,
    message_board as db_mb,
    state::AppState,
    tasks as db_tasks,
};

/// Start the HTTP + WebSocket server on an already-bound listener. Runs forever.
pub async fn serve(listener: TcpListener, state: AppState) -> Result<()> {
    let addr = listener.local_addr()?;
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/ws", get(ws_handler))
        .with_state(state);

    info!("HTTP+WebSocket server listening on {addr}");

    axum::serve(listener, app).await?;
    Ok(())
}

async fn health_handler() -> &'static str {
    "ok"
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let agent_id = params.get("agent_id").cloned().unwrap_or_default();
    ws.on_upgrade(move |socket| handle_connection(socket, agent_id, state))
}

async fn handle_connection(socket: WebSocket, agent_id: String, state: AppState) {
    if agent_id.is_empty() {
        warn!("Connection rejected: missing agent_id");
        return;
    }

    info!("Agent '{agent_id}' connected");

    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    // Register in the agent registry with defaults — agent.register will set real values.
    // registered=false: try_dispatch won't dispatch to this agent until agent.register
    // is processed, preserving backward compat with agents that use task.get_next.
    {
        let entry = AgentState {
            id: agent_id.clone(),
            tags: vec![],
            capacity_max: 1,
            active_tasks: 0,
            last_seen_at: Utc::now(),
            ws_tx: tx,
            registered: false,
        };
        if let Ok(mut agents) = state.agents.lock() {
            agents.insert(agent_id.clone(), entry);
        }
    }

    let (mut ws_sink, mut ws_stream) = socket.split();

    // Forward outbound messages from the channel to the WebSocket.
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Receive inbound WebSocket messages and dispatch them.
    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                dispatch(&agent_id, &text, &state).await;
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }

    send_task.abort();

    // On disconnect: reset any in-progress tasks assigned to this agent.
    match db_tasks::reset_in_progress_for_agent(&state.db, &agent_id) {
        Ok(n) if n > 0 => {
            info!("Agent '{agent_id}' disconnected: reset {n} in-progress task(s) to pending");
            broadcast_tasks(&state);
        }
        Ok(_) => {}
        Err(e) => warn!("Failed to reset tasks for '{agent_id}' on disconnect: {e}"),
    }

    // Remove from registry.
    if let Ok(mut agents) = state.agents.lock() {
        agents.remove(&agent_id);
    }

    // Update last_seen_at in DB.
    if let Err(e) = communication::touch_agent(&state.db, &agent_id) {
        warn!("Failed to update agent last_seen on disconnect: {e}");
    }

    info!("Agent '{agent_id}' disconnected");
    broadcast_agents(&state);
}

async fn dispatch(agent_id: &str, text: &str, state: &AppState) {
    let msg: ApiMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            warn!("Malformed message from {agent_id}: {e}");
            return;
        }
    };

    if msg.msg_type != MessageType::Request {
        return;
    }

    let method = msg.method.as_deref().unwrap_or("");
    let response = match method {
        "ping" => make_response(&msg.id, serde_json::json!({ "pong": true })),
        "task.create" => handle(&msg.id, handlers::tasks::create(&state.db, msg.params)),
        "task.list"   => handle(&msg.id, handlers::tasks::list(&state.db, msg.params)),
        "task.get"    => handle(&msg.id, handlers::tasks::get(&state.db, msg.params)),
        "task.update"    => handle(&msg.id, handlers::tasks::update(&state.db, msg.params)),
        "task.get_next"      => handle(&msg.id, handlers::tasks::get_next(&state.db, agent_id, msg.params)),
        "task.complete"      => handle(&msg.id, handlers::tasks::complete(&state.db, agent_id, msg.params)),
        "task.split"         => handle(&msg.id, handlers::tasks::split(&state.db, msg.params)),
        "task.set_dependency"=> handle(&msg.id, handlers::tasks::set_dependency(&state.db, msg.params)),
        "topic.create"  => handle(&msg.id, handlers::message_board::create(&state.db, msg.params)),
        "topic.list"    => handle(&msg.id, handlers::message_board::list(&state.db, msg.params)),
        "topic.list_new" => handle(&msg.id, handlers::message_board::list_new(&state.db, msg.params)),
        "topic.get"     => handle(&msg.id, handlers::message_board::get(&state.db, msg.params)),
        "topic.comment" => handle(&msg.id, handlers::message_board::comment(&state.db, &state.agents, msg.params)),
        "topic.wait"    => handle(&msg.id, handlers::message_board::wait(&state.db, msg.params).await),
        "agent.register"  => handle(&msg.id, handlers::agents::register(&state.db, &state.agents, msg.params)),
        "agent.list"      => handle(&msg.id, handlers::agents::list(&state.db)),
        "agent.status"    => handle(&msg.id, handlers::agents::status(&state.agents, &state.db, agent_id, msg.params)),
        "agent.heartbeat" => handle(&msg.id,
            communication::touch_agent(&state.db, agent_id)
                .map(|_| serde_json::json!({ "ok": true }))
        ),
        "push.send" => handle(
            &msg.id,
            handlers::push::send(&state.db, &state.agents, agent_id, msg.params),
        ),
        "push.list" => handle(&msg.id, handlers::push::list(&state.db, agent_id)),
        "push.ack"  => handle(&msg.id, handlers::push::ack(&state.db, msg.params)),
        _ => make_error(&msg.id, 404, format!("unknown method: {method}")),
    };

    // Broadcast state changes to all connected clients on successful mutations.
    if response.error.is_none() {
        match method {
            "task.create" | "task.update" | "task.complete" | "task.split"
            | "task.set_dependency" => broadcast_tasks(state),
            "task.get_next" => {
                if response.result.as_ref().map(|v| !v.is_null()).unwrap_or(false) {
                    broadcast_tasks(state);
                }
            }
            "agent.register" | "agent.heartbeat" | "agent.status" => broadcast_agents(state),
            "topic.create" | "topic.comment" => broadcast_topics(state),
            _ => {}
        }
    }

    // Send response first so task.assign push (if any) arrives after the response.
    agent_registry::send_to_agent(&state.agents, agent_id, &response);

    // Post-send dispatch triggers (must be after send_to_agent so the response
    // reaches the agent before any task.assign push message).
    if response.error.is_none() {
        match method {
            // agent.register: agent just became eligible for dispatch.
            // agent.status: agent reported new active_tasks count; may have capacity.
            // task.create | task.split: new tasks available for idle agents.
            "agent.register" | "agent.status" | "task.create" | "task.split" => {
                agent_registry::try_dispatch(&state.agents, &state.db);
            }
            // task.complete: agent will send agent.status { active_tasks: N } shortly,
            // which triggers dispatch. No eager dispatch here.
            _ => {}
        }
    }
}

pub fn make_push(payload: serde_json::Value) -> ApiMessage {
    ApiMessage {
        msg_type: MessageType::Push,
        id: Uuid::new_v4().to_string(),
        method: Some("push".to_string()),
        params: Some(payload),
        result: None,
        error: None,
    }
}

fn handle(id: &str, result: anyhow::Result<serde_json::Value>) -> ApiMessage {
    match result {
        Ok(v) => make_response(id, v),
        Err(e) => make_error(id, 500, e.to_string()),
    }
}

fn make_response(id: &str, result: serde_json::Value) -> ApiMessage {
    ApiMessage {
        msg_type: MessageType::Response,
        id: id.to_string(),
        method: None,
        params: None,
        result: Some(result),
        error: None,
    }
}

fn make_error(id: &str, code: i32, message: String) -> ApiMessage {
    ApiMessage {
        msg_type: MessageType::Error,
        id: id.to_string(),
        method: None,
        params: None,
        result: None,
        error: Some(ApiError { code, message }),
    }
}

fn broadcast(method: &str, payload: serde_json::Value, state: &AppState) {
    let msg = ApiMessage {
        msg_type: MessageType::Push,
        id: Uuid::new_v4().to_string(),
        method: Some(method.to_string()),
        params: Some(payload),
        result: None,
        error: None,
    };
    agent_registry::broadcast_all(&state.agents, &msg);
}

fn broadcast_tasks(state: &AppState) {
    match db_tasks::list_tasks(&state.db, None, None, None) {
        Ok(tasks) => match serde_json::to_value(&tasks) {
            Ok(v) => broadcast("tasks.updated", v, state),
            Err(e) => warn!("broadcast_tasks serialize error: {e}"),
        },
        Err(e) => warn!("broadcast_tasks query error: {e}"),
    }
}

fn broadcast_topics(state: &AppState) {
    match db_mb::list_topics(&state.db) {
        Ok(topics) => match serde_json::to_value(&topics) {
            Ok(v) => broadcast("topics.updated", v, state),
            Err(e) => warn!("broadcast_topics serialize error: {e}"),
        },
        Err(e) => warn!("broadcast_topics query error: {e}"),
    }
}

fn broadcast_agents(state: &AppState) {
    match communication::list_agents(&state.db) {
        Ok(agents) => match serde_json::to_value(&agents) {
            Ok(v) => broadcast("agents.updated", v, state),
            Err(e) => warn!("broadcast_agents serialize error: {e}"),
        },
        Err(e) => warn!("broadcast_agents query error: {e}"),
    }
}
