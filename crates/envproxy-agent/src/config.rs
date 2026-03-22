//! Configuration for the envproxy agent.
//!
//! The agent reads its configuration from a TOML file. Example:
//!
//! ```toml
//! [agent]
//! socket = "/tmp/envproxy/agent.sock"
//! log_level = "info"
//!
//! [backend]
//! type = "file"
//! path = "/etc/envproxy/secrets.json"
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::backend::http::HttpBackendConfig;
use envproxy_proto::DEFAULT_SOCKET_PATH;

/// Top-level agent configuration.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// Agent settings.
    #[serde(default)]
    pub agent: AgentConfig,

    /// Backend configuration.
    pub backend: BackendConfig,
}

/// Agent daemon settings.
#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    /// Path to the Unix socket.
    #[serde(default = "default_socket_path")]
    pub socket: PathBuf,

    /// Tracing log level (e.g., "info", "debug", "trace").
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self { socket: PathBuf::from(DEFAULT_SOCKET_PATH), log_level: default_log_level() }
    }
}

fn default_socket_path() -> PathBuf {
    PathBuf::from(DEFAULT_SOCKET_PATH)
}

fn default_log_level() -> String {
    "info".to_owned()
}

/// Backend configuration — determines where secrets are fetched from.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum BackendConfig {
    /// Read secrets from a JSON file on disk.
    File {
        /// Path to the JSON secrets file.
        path: PathBuf,
    },

    /// Fetch secrets from a remote HTTP API.
    Http(HttpBackendConfig),
}

impl Config {
    /// Load configuration from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let contents =
            std::fs::read_to_string(path).map_err(|e| ConfigError::Io(path.to_path_buf(), e))?;
        let config: Self =
            toml::from_str(&contents).map_err(|e| ConfigError::Parse(path.to_path_buf(), e))?;
        Ok(config)
    }
}

/// Errors that can occur when loading configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The configuration file could not be read.
    #[error("failed to read config file {0}: {1}")]
    Io(PathBuf, std::io::Error),

    /// The configuration file could not be parsed.
    #[error("failed to parse config file {0}: {1}")]
    Parse(PathBuf, toml::de::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file_backend_config() {
        let toml_str = r#"
            [agent]
            socket = "/tmp/envproxy/test.sock"
            log_level = "debug"

            [backend]
            type = "file"
            path = "/etc/envproxy/secrets.json"
        "#;

        let config: Config = toml::from_str(toml_str).expect("should parse valid TOML");
        assert_eq!(config.agent.socket, PathBuf::from("/tmp/envproxy/test.sock"));
        assert_eq!(config.agent.log_level, "debug");
        assert!(matches!(config.backend, BackendConfig::File { .. }));
    }

    #[test]
    fn parse_http_backend_config() {
        let toml_str = r#"
            [backend]
            type = "http"
            url = "https://secrets.internal"
            auth_token = "my-token"
        "#;

        let config: Config = toml::from_str(toml_str).expect("should parse valid TOML");
        assert!(matches!(config.backend, BackendConfig::Http(_)));
    }

    #[test]
    fn default_agent_config_should_use_standard_socket_path() {
        let config = AgentConfig::default();
        assert_eq!(config.socket, PathBuf::from(DEFAULT_SOCKET_PATH));
        assert_eq!(config.log_level, "info");
    }
}
