//! Backend trait and implementations for resolving environment variable values.
//!
//! Each backend implements the [`Backend`] trait, which provides a single
//! `resolve` method that takes a key and returns an optional value.

pub mod file;
pub mod http;
#[cfg(feature = "kubernetes")]
pub mod kubernetes;
#[cfg(feature = "vault")]
pub mod vault;

use std::future::Future;
use std::pin::Pin;

/// Errors that can occur during backend resolution.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    /// The backend is not configured or not available.
    #[error("backend unavailable: {0}")]
    Unavailable(String),

    /// An I/O error occurred while reading from the backend source.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// An HTTP error occurred while communicating with the backend.
    #[error("HTTP error: {0}")]
    #[cfg_attr(
        not(feature = "vault"),
        expect(dead_code, reason = "used by vault and http backends when enabled")
    )]
    Http(String),

    /// The backend returned data that could not be parsed.
    #[error("parse error: {0}")]
    Parse(String),
}

/// A backend that can resolve environment variable keys to values.
///
/// Implementations might read from files, HTTP APIs, Vault, AWS Secrets Manager, etc.
///
/// The `resolve` method takes `&self` and an owned `String` key. We use an owned
/// key rather than `&str` because the future returned by `resolve` must be `Send`
/// and may outlive the caller's borrow of the key.
pub trait Backend: Send + Sync {
    /// Resolve an environment variable key to its value.
    ///
    /// Returns `Ok(Some(value))` if the key was found,
    /// `Ok(None)` if the key does not exist in this backend,
    /// or `Err(...)` if an error occurred during resolution.
    fn resolve(
        &self,
        key: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, BackendError>> + Send + 'static>>;

    /// Resolve a key using the current env var value (v2 protocol).
    ///
    /// When the env var value starts with `vault:`, this method is called
    /// instead of `resolve()`. The value contains the Vault path reference
    /// (e.g., `vault:secret/data/myapp/config#DATABASE_URL`).
    ///
    /// Default implementation falls back to `resolve()` (ignores the value).
    fn resolve_with_value(
        &self,
        key: &str,
        _value: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, BackendError>> + Send + 'static>> {
        self.resolve(key)
    }
}
