//! HTTP-based backend for resolving environment variables from a remote API.
//!
//! This backend calls a remote HTTP API to resolve environment variable values.
//! The API contract is simple:
//!
//! ```text
//! GET /v1/env/{key}
//! Headers: Authorization: Bearer <token>
//! Response: { "key": "DATABASE_URL", "value": "postgres://...", "ttl": 300 }
//! ```
//!
//! This module provides the backend trait implementation. The actual HTTP client
//! is a future enhancement — this module defines the API contract and types.

use std::future::Future;
use std::pin::Pin;

use serde::Deserialize;

use super::{Backend, BackendError};

/// Response from the HTTP secrets API.
///
/// This type defines the expected JSON response shape from a conforming
/// secrets API. It will be used when the HTTP backend is fully implemented.
#[derive(Debug, Deserialize)]
#[expect(dead_code, reason = "defines the API contract; used when HTTP backend is implemented")]
pub struct SecretResponse {
    /// The environment variable key.
    pub key: String,
    /// The resolved value.
    pub value: String,
    /// Time-to-live in seconds (how long to cache this value).
    pub ttl: Option<u64>,
}

/// Configuration for the HTTP backend.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct HttpBackendConfig {
    /// Base URL of the secrets API (e.g., `https://secrets.internal`).
    pub url: String,
    /// Path to a file containing the auth token, or the token itself.
    pub auth_token: Option<String>,
    /// Request timeout in milliseconds.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

const fn default_timeout_ms() -> u64 {
    5000
}

/// HTTP backend that resolves keys from a remote secrets API.
///
/// This is a placeholder implementation. A full implementation would use
/// `reqwest` or `hyper` to make HTTP requests to the configured API.
pub struct HttpBackend {
    config: HttpBackendConfig,
}

impl HttpBackend {
    /// Create a new HTTP backend with the given configuration.
    #[must_use]
    pub fn new(config: HttpBackendConfig) -> Self {
        Self { config }
    }
}

impl Backend for HttpBackend {
    fn resolve(
        &self,
        key: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, BackendError>> + Send + 'static>> {
        let key = key.to_owned();
        let url = self.config.url.clone();
        Box::pin(async move {
            // Placeholder: a real implementation would make an HTTP request here.
            // For now, log the intent and return an error.
            tracing::debug!(
                url = %url,
                key = %key,
                "HTTP backend resolve (not yet implemented)"
            );
            Err(BackendError::Unavailable(format!(
                "HTTP backend not yet implemented (url: {url}, key: {key})"
            )))
        })
    }
}
