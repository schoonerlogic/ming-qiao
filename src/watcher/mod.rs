//! Watcher system for real-time event observation.
//!
//! Watchers are observer agents (like Laozi-Jung) that receive real-time event
//! streams from ming-qiao without polling. Each watcher subscribes to NATS-style
//! subject patterns and dispatches matching events via a configured action.
//!
//! ## Configuration
//!
//! Add `[[watchers]]` entries to `ming-qiao.toml`:
//!
//! ```toml
//! [[watchers]]
//! agent = "laozi-jung"
//! role = "observer"
//! subjects = ["am.events.mingqiao"]
//!
//! [watchers.action]
//! type = "file_append"
//! path = "/path/to/stream.jsonl"
//! ```

pub mod actions;
pub mod config;
pub mod dispatch;
pub mod subjects;

pub use config::{WatcherAction, WatcherConfig, WatcherFilter, WatcherRole};
pub use dispatch::{warn_observer_write, WatcherDispatcher};
pub use subjects::{matches_subject, subjects_for_event};
