//! MCP (Model Context Protocol) HTTP server.
//!
//! Exposes coordination tools to the coding agent via JSON-RPC 2.0 over HTTP.
//! Listens on localhost:7890 by default.

pub mod rpc;
pub mod server;
pub mod tools;
