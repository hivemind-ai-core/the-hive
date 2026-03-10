//! Hive Core - Shared types for The Hive swarm orchestration system.
//!
//! This crate contains common types used across hive-cli, hive-server,
//! hive-agent, and app-daemon.

pub mod error;
pub mod types;

pub use error::Error;
pub use types::*;
