//! Event types for agent communication
//!
//! This module defines the core event schemas used for message passing
//! between Council agents. Event persistence is handled by the SurrealDB
//! layer in `db::persistence`.

mod schema;

pub mod error;

#[cfg(test)]
mod tests;

pub use schema::*;

// Re-export error type
pub use error::EventError;
