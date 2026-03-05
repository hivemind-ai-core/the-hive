//! Shared application state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::ws::Message;
use tokio::sync::mpsc::UnboundedSender;

use crate::db::DbPool;

/// A handle to send WebSocket messages to a connected agent.
pub type AgentSender = UnboundedSender<Message>;

/// Live connected agents: agent_id → send channel.
pub type Clients = Arc<Mutex<HashMap<String, AgentSender>>>;

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub clients: Clients,
}

impl AppState {
    pub fn new(db: DbPool) -> Self {
        Self {
            db,
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
