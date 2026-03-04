//! HTTP server implementation
//!
//! Creates and runs the Axum HTTP server with all routes configured.

use std::net::SocketAddr;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::http::routes;
use crate::state::AppState;

/// Default server port
pub const DEFAULT_PORT: u16 = 7777;

/// HTTP server configuration
#[derive(Debug, Clone)]
pub struct HttpServerConfig {
    /// Port to listen on
    pub port: u16,

    /// Host to bind to (default: 127.0.0.1 for local-only)
    pub host: String,

    /// Enable CORS for development
    pub enable_cors: bool,
}

impl Default for HttpServerConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            host: "127.0.0.1".to_string(),
            enable_cors: true,
        }
    }
}

/// HTTP server for ming-qiao
pub struct HttpServer {
    config: HttpServerConfig,
    state: AppState,
}

impl HttpServer {
    /// Create a new HTTP server with default configuration and state
    pub fn new(state: AppState) -> Self {
        Self {
            config: HttpServerConfig::default(),
            state,
        }
    }

    /// Create a new HTTP server with custom configuration
    pub fn with_config(config: HttpServerConfig, state: AppState) -> Self {
        Self { config, state }
    }

    /// Build the router with all routes and shared state
    pub fn router(&self) -> Router {
        let mut app = Router::new()
            .merge(routes::api_routes(self.state.clone()))
            .with_state(self.state.clone())
            .layer(TraceLayer::new_for_http());

        if self.config.enable_cors {
            app = app.layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods(Any)
                    .allow_headers(Any),
            );
        }

        app
    }

    /// Run the HTTP server
    pub async fn run(&self) -> Result<(), std::io::Error> {
        let addr: SocketAddr = format!("{}:{}", self.config.host, self.config.port)
            .parse()
            .expect("Invalid address");

        info!("HTTP server starting on http://{}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, self.router()).await?;

        Ok(())
    }

    /// Get the server address
    pub fn address(&self) -> String {
        format!("{}:{}", self.config.host, self.config.port)
    }

    /// Get a reference to the application state
    pub fn state(&self) -> &AppState {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HttpServerConfig::default();
        assert_eq!(config.port, 7777);
        assert_eq!(config.host, "127.0.0.1");
        assert!(config.enable_cors);
    }

    #[tokio::test]
    async fn test_server_address() {
        let state = AppState::new().await;
        let server = HttpServer::new(state);
        assert_eq!(server.address(), "127.0.0.1:7777");
    }

    #[tokio::test]
    async fn test_router_builds() {
        let state = AppState::new().await;
        let server = HttpServer::new(state);
        let _router = server.router();
    }
}
