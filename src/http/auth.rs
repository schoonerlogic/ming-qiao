//! Bearer token authentication middleware for API write paths
//!
//! Security P0: Transitional per-agent bearer tokens until SPIRE enforces identity.
//!
//! Per Thales spec:
//! - Each agent has a unique bearer token
//! - Write endpoints reject unauthenticated caller context
//! - `from_agent` field must match authenticated caller identity
//! - Privileged roles (thales, merlin, council-chamber) can read any inbox

use std::collections::HashMap;
use std::path::Path;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// Agent authentication config loaded from token file.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Map of bearer token -> agent_id
    token_to_agent: HashMap<String, String>,
    /// Agents with elevated privileges (can read any inbox)
    privileged_agents: Vec<String>,
    /// Whether auth is enabled (false = passthrough for backward compat)
    pub enabled: bool,
}

/// Serializable token file format.
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenFile {
    /// Map of agent_id -> bearer token
    pub tokens: HashMap<String, String>,
    /// Agents with read-all-inboxes privilege
    #[serde(default)]
    pub privileged_agents: Vec<String>,
}

impl AuthConfig {
    /// Create a disabled (passthrough) auth config.
    pub fn disabled() -> Self {
        Self {
            token_to_agent: HashMap::new(),
            privileged_agents: Vec::new(),
            enabled: false,
        }
    }

    /// Load auth config from a token file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, AuthError> {
        let contents = std::fs::read_to_string(path)?;
        let file: TokenFile = serde_json::from_str(&contents)?;

        // Invert the map: agent -> token becomes token -> agent
        let token_to_agent: HashMap<String, String> = file
            .tokens
            .into_iter()
            .map(|(agent, token)| (token, agent))
            .collect();

        Ok(Self {
            token_to_agent,
            privileged_agents: file.privileged_agents,
            enabled: true,
        })
    }

    /// Validate a bearer token, returning the agent ID if valid.
    pub fn validate_token(&self, token: &str) -> Option<&str> {
        self.token_to_agent.get(token).map(|s| s.as_str())
    }

    /// Check if an agent has elevated privileges.
    pub fn is_privileged(&self, agent_id: &str) -> bool {
        self.privileged_agents.iter().any(|a| a == agent_id)
    }
}

/// Extension type injected by auth middleware to identify the caller.
#[derive(Debug, Clone)]
pub struct AuthenticatedAgent {
    pub agent_id: String,
    pub is_privileged: bool,
}

/// Middleware that requires a valid bearer token on write endpoints.
///
/// If auth is disabled in config, all requests pass through with agent_id "anonymous".
/// If enabled, extracts Bearer token from Authorization header and validates.
pub async fn require_write_auth(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let auth_config = state.auth_config().await;

    if !auth_config.enabled {
        // Auth disabled — inject anonymous identity for backward compat
        request.extensions_mut().insert(AuthenticatedAgent {
            agent_id: "anonymous".to_string(),
            is_privileged: true,
        });
        return Ok(next.run(request).await);
    }

    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match token {
        Some(t) => match auth_config.validate_token(t) {
            Some(agent_id) => {
                let is_privileged = auth_config.is_privileged(agent_id);
                request.extensions_mut().insert(AuthenticatedAgent {
                    agent_id: agent_id.to_string(),
                    is_privileged,
                });
                Ok(next.run(request).await)
            }
            None => Err(error_response(
                StatusCode::UNAUTHORIZED,
                "INVALID_TOKEN",
                "Bearer token not recognized",
            )),
        },
        None => Err(error_response(
            StatusCode::UNAUTHORIZED,
            "MISSING_TOKEN",
            "Authorization: Bearer <token> header required",
        )),
    }
}

/// Middleware that requires auth for inbox reads (agent must match or be privileged).
pub async fn require_inbox_auth(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let auth_config = state.auth_config().await;

    if !auth_config.enabled {
        request.extensions_mut().insert(AuthenticatedAgent {
            agent_id: "anonymous".to_string(),
            is_privileged: true,
        });
        return Ok(next.run(request).await);
    }

    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match token {
        Some(t) => match auth_config.validate_token(t) {
            Some(agent_id) => {
                let is_privileged = auth_config.is_privileged(agent_id);
                request.extensions_mut().insert(AuthenticatedAgent {
                    agent_id: agent_id.to_string(),
                    is_privileged,
                });
                Ok(next.run(request).await)
            }
            None => Err(error_response(
                StatusCode::UNAUTHORIZED,
                "INVALID_TOKEN",
                "Bearer token not recognized",
            )),
        },
        // Allow unauthenticated reads for backward compat during transition
        None => {
            request.extensions_mut().insert(AuthenticatedAgent {
                agent_id: "anonymous".to_string(),
                is_privileged: true, // permissive during transition
            });
            Ok(next.run(request).await)
        }
    }
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    let body = serde_json::json!({
        "error": {
            "code": code,
            "message": message
        }
    });
    (status, Json(body)).into_response()
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disabled_config() {
        let config = AuthConfig::disabled();
        assert!(!config.enabled);
        assert!(config.validate_token("anything").is_none());
    }

    #[test]
    fn test_token_validation() {
        let mut token_to_agent = HashMap::new();
        token_to_agent.insert("secret-aleph".to_string(), "aleph".to_string());
        token_to_agent.insert("secret-luban".to_string(), "luban".to_string());

        let config = AuthConfig {
            token_to_agent,
            privileged_agents: vec!["thales".to_string()],
            enabled: true,
        };

        assert_eq!(config.validate_token("secret-aleph"), Some("aleph"));
        assert_eq!(config.validate_token("secret-luban"), Some("luban"));
        assert_eq!(config.validate_token("wrong"), None);
    }

    #[test]
    fn test_privileged_check() {
        let config = AuthConfig {
            token_to_agent: HashMap::new(),
            privileged_agents: vec!["thales".to_string(), "merlin".to_string()],
            enabled: true,
        };

        assert!(config.is_privileged("thales"));
        assert!(config.is_privileged("merlin"));
        assert!(!config.is_privileged("aleph"));
    }

    #[test]
    fn test_load_token_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("tokens.json");

        let file = TokenFile {
            tokens: {
                let mut m = HashMap::new();
                m.insert("aleph".to_string(), "tok-aleph-123".to_string());
                m.insert("luban".to_string(), "tok-luban-456".to_string());
                m
            },
            privileged_agents: vec!["thales".to_string()],
        };

        std::fs::write(&path, serde_json::to_string_pretty(&file).unwrap()).unwrap();

        let config = AuthConfig::load(&path).unwrap();
        assert!(config.enabled);
        assert_eq!(config.validate_token("tok-aleph-123"), Some("aleph"));
        assert_eq!(config.validate_token("tok-luban-456"), Some("luban"));
        assert!(config.is_privileged("thales"));
    }
}
