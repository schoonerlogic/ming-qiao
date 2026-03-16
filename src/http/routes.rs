//! HTTP route definitions
//!
//! Defines all API routes and connects them to handlers.
//! Write paths are protected by bearer token auth middleware (Security P0).

use axum::{
    middleware,
    routing::{delete, get, patch, post},
    Router,
};

use crate::http::{auth, handlers, merlin, ws};
use crate::mcp::streamable_http;
use crate::state::AppState;

/// Create the API router with all routes.
///
/// Write paths (POST, PATCH) are protected by `require_write_auth` middleware.
/// Read paths remain open during the transitional period, with optional auth
/// via `require_inbox_auth` for inbox endpoints.
pub fn api_routes(state: AppState) -> Router<AppState> {
    // Write routes — require bearer token auth
    let write_routes = Router::new()
        // Thread creation and replies
        .route("/api/threads", post(handlers::create_thread))
        .route("/api/thread/:id/reply", post(handlers::reply_to_thread))
        .route("/api/thread/:id", patch(handlers::update_thread))
        // Message updates
        .route("/api/message/:id", patch(handlers::update_message))
        // Decision actions
        .route(
            "/api/decisions/:id/approve",
            post(handlers::approve_decision),
        )
        .route("/api/decisions/:id/reject", post(handlers::reject_decision))
        // Merlin actions
        .route("/api/inject", post(handlers::inject_message))
        .route("/api/annotate", post(handlers::add_annotation))
        .route("/api/config", patch(handlers::update_config))
        // Observations
        .route("/api/observe", post(handlers::submit_observation))
        // Admin
        .route("/api/admin/rehydrate", post(handlers::rehydrate_indexer))
        // Apply write auth middleware to all routes in this group
        .route_layer(middleware::from_fn_with_state(state, auth::require_write_auth));

    // Signed event route — uses Ed25519 envelope verification (RA-004), not bearer tokens
    let signed_routes = Router::new()
        .route("/api/signed-event", post(handlers::submit_signed_event));

    // Read routes — no auth required during transition (P1 will add inbox auth)
    let read_routes = Router::new()
        // Health check
        .route("/health", get(handlers::health_check))
        // WebSocket for real-time events
        .route("/ws", get(ws::ws_handler))
        // Merlin notification stream
        .route(
            "/merlin/notifications",
            get(merlin::merlin_notifications_ws),
        )
        // Inbox (transitional: open reads, P1 adds per-agent auth)
        .route("/api/inbox/:agent", get(handlers::get_inbox))
        .route("/api/inbox/:agent/ack", post(handlers::acknowledge_inbox))
        // Threads (read)
        .route("/api/threads", get(handlers::list_threads))
        .route("/api/thread/:id", get(handlers::get_thread))
        // Messages (read)
        .route("/api/message/:id", get(handlers::get_message))
        // Artifacts
        .route("/api/artifacts", get(handlers::list_artifacts))
        .route("/api/artifacts/*path", get(handlers::get_artifact))
        // Decisions (read)
        .route("/api/decisions", get(handlers::list_decisions))
        .route("/api/decisions/:id", get(handlers::get_decision))
        // Config (read)
        .route("/api/config", get(handlers::get_config))
        // Search
        .route("/api/search", get(handlers::search))
        // Read cursors (for am-fleet comms)
        .route("/api/cursors", get(handlers::get_cursors))
        // MCP Streamable HTTP transport (Phase 2)
        .route("/mcp", post(streamable_http::handle_post)
            .get(streamable_http::handle_get)
            .delete(streamable_http::handle_delete));

    read_routes.merge(write_routes).merge(signed_routes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_routes_build() {
        let state = AppState::new().await;
        let _router = api_routes(state);
        // Just verify routes build without panic
    }
}
