//! Event types for agent communication
//!
//! This module defines the core event schemas used for message passing
//! between Council agents, along with persistence functionality.

mod schema;

pub mod error;
pub mod reader;
pub mod writer;

#[cfg(test)]
mod tests;

pub use schema::*;

// Re-export persistence types
pub use error::EventError;
pub use reader::{EventReader, ReplayIterator, TailIterator};
pub use writer::EventWriter;
