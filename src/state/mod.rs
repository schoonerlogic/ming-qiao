//! Shared application state for ming-qiao
//!
//! This module provides the central state that is shared between the MCP server
//! and HTTP gateway. It manages access to the event log, configuration, and
//! provides thread-safe operations for reading and writing events.

mod app_state;
mod config;

pub use app_state::AppState;
pub use config::{Config, DatabaseAuthLevel, NatsAuthMode, NatsConfig, ObservationMode};
