//! HTTP + WebSocket server: /health for Docker healthcheck, /ws for agents.

use std::collections::HashMap;

use anyhow::Result;
use axum::{
    Router,
    extract::{Query, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use hive_core::types::{ApiError, ApiMessage, MessageType};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use uuid::Uuid;
use tracing::{info, warn};

use crate::{communication, handlers, message_board as db_mb, state::AppState, tasks as db_tasks};

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

    match state.clients.lock() {
        Ok(mut clients) => { clients.insert(agent_id.clone(), tx); }
        Err(e) => {
            warn!("Client lock poisoned on connect for '{agent_id}': {e}");
            return;
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
    match state.clients.lock() {
        Ok(mut clients) => { clients.remove(&agent_id); }
        Err(e) => warn!("Client lock poisoned on disconnect for '{agent_id}': {e}"),
    }
    // Mark agent as disconnected in the DB.
    if let Err(e) = crate::communication::touch_agent(&state.db, &agent_id) {
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
        "topic.comment" => handle(&msg.id, handlers::message_board::comment(&state.db, msg.params)),
        "topic.wait"    => handle(&msg.id, handlers::message_board::wait(&state.db, msg.params).await),
        "agent.register" => handle(&msg.id, handlers::agents::register(&state.db, msg.params)),
        "agent.list"     => handle(&msg.id, handlers::agents::list(&state.db)),
        "push.send" => handle(
            &msg.id,
            handlers::push::send(&state.db, &state.clients, agent_id, msg.params),
        ),
        "push.list" => handle(&msg.id, handlers::push::list(&state.db, agent_id)),
        "push.ack"  => handle(&msg.id, handlers::push::ack(&state.db, msg.params)),
        _ => make_error(&msg.id, 404, format!("unknown method: {method}")),
    };

    // Broadcast state changes to all connected clients on successful mutations.
    if response.error.is_none() {
        match method {
            "task.create" | "task.update" | "task.complete" | "task.split"
            | "task.get_next" | "task.set_dependency" => broadcast_tasks(state),
            "agent.register" => broadcast_agents(state),
            "topic.create" | "topic.comment" => broadcast_topics(state),
            _ => {}
        }
    }

    send_to(agent_id, response, state);
}

fn send_to(agent_id: &str, msg: ApiMessage, state: &AppState) {
    match state.clients.lock() {
        Ok(clients) => {
            if let Some(tx) = clients.get(agent_id) {
                if let Ok(json) = serde_json::to_string(&msg) {
                    let _ = tx.send(Message::Text(json.into()));
                }
            }
        }
        Err(e) => warn!("Client lock poisoned in send_to '{agent_id}': {e}"),
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
    if let Ok(json) = serde_json::to_string(&msg) {
        if let Ok(clients) = state.clients.lock() {
            for tx in clients.values() {
                let _ = tx.send(Message::Text(json.clone().into()));
            }
        }
    }
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

