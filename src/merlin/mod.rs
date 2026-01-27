//! Merlin (Proteus) notification and intervention system
//!
//! Provides notifications to the human operator based on observation mode
//! and enables direct intervention via message injection.

pub mod notifier;

pub use notifier::{MerlinNotification, MerlinNotifier};
