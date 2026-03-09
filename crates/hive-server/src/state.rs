//! Shared application state.

use crate::agent_registry::{new_registry, AgentRegistry};
use crate::db::DbPool;

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    /// In-memory registry of connected agents and their state.
    pub agents: AgentRegistry,
}

impl AppState {
    pub fn new(db: DbPool) -> Self {
        Self {
            db,
            agents: new_registry(),
        }
    }
}
