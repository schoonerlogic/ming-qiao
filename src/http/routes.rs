//! HTTP route definitions
//!
//! Defines all API routes and connects them to handlers.

use axum::{
    routing::{get, patch, post},
    Router,
};

use crate::http::{handlers, merlin, ws};
use crate::state::AppState;

/// Create the API router with all routes
pub fn api_routes() -> Router<AppState> {
    Router::new()
        // Health check
        .route("/health", get(handlers::health_check))
        // WebSocket for real-time events
        .route("/ws", get(ws::ws_handler))
        // Merlin notification stream
        .route(
            "/merlin/notifications",
            get(merlin::merlin_notifications_ws),
        )
        // Inbox
        .route("/api/inbox/:agent", get(handlers::get_inbox))
        .route("/api/inbox/:agent/unread", get(handlers::get_unread_count))
        // Threads
        .route("/api/threads", get(handlers::list_threads))
        .route("/api/threads", post(handlers::create_thread))
        .route("/api/thread/:id", get(handlers::get_thread))
        .route("/api/thread/:id", patch(handlers::update_thread))
        .route("/api/thread/:id/reply", post(handlers::reply_to_thread))
        // Messages
        .route("/api/message/:id", get(handlers::get_message))
        .route("/api/message/:id", patch(handlers::update_message))
        // Artifacts
        .route("/api/artifacts", get(handlers::list_artifacts))
        .route("/api/artifacts/*path", get(handlers::get_artifact))
        // Decisions
        .route("/api/decisions", get(handlers::list_decisions))
        .route("/api/decisions/:id", get(handlers::get_decision))
        .route(
            "/api/decisions/:id/approve",
            post(handlers::approve_decision),
        )
        .route("/api/decisions/:id/reject", post(handlers::reject_decision))
        // Merlin actions
        .route("/api/inject", post(handlers::inject_message))
        .route("/api/annotate", post(handlers::add_annotation))
        .route("/api/config", get(handlers::get_config))
        .route("/api/config", patch(handlers::update_config))
        // Observations (Laozi-Jung return path)
        .route("/api/observe", post(handlers::submit_observation))
        // Search
        .route("/api/search", get(handlers::search))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routes_build() {
        let _router = api_routes();
        // Just verify routes build without panic
    }
}
