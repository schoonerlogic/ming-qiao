//! Ming-Qiao: Communication bridge for the Council of Wizards
//!
//! Enables agents to exchange messages without copy-paste intermediation.
//! Persists all exchanges for decision archaeology.
//!
//! ## Architecture
//!
//! - `events` - Event schema definitions (append-only log format)
//! - `mcp` - MCP server for Aleph (Claude CLI) via stdio
//! - `db` - Database models for SurrealDB (materialized views)
//! - `http` - HTTP/REST API for Thales and Merlin dashboard
//! - `state` - Shared application state and configuration
//! - `merlin` - Merlin (human operator) notification system

pub mod db;
pub mod events;
pub mod http;
pub mod mcp;
pub mod merlin;
pub mod nats;
pub mod state;
