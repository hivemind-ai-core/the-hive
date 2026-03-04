//! Hive Core - Shared types for The Hive swarm orchestration system.
//!
//! This crate contains common types used across hive-cli, hive-server,
//! hive-agent, and app-daemon.

pub mod types;
pub mod error;

pub use types::*;
pub use error::Error;
