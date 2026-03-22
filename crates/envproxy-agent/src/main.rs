//! `envproxy-agent` — local daemon that resolves environment variables from backends.
//!
//! The agent listens on a Unix socket and handles requests from `libenvproxy.so`.
//! It reads its configuration from a TOML file and resolves keys via the
//! configured backend (file, HTTP, etc.).
//!
//! # Usage
//!
//! ```bash
//! envproxy-agent --config /etc/envproxy/config.toml
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod backend;
mod config;
mod server;

use config::{BackendConfig, Config};

/// envproxy-agent: local daemon for dynamic environment variable resolution.
#[derive(Parser)]
#[command(name = "envproxy-agent")]
#[command(about = "Local daemon that resolves environment variables from configured backends")]
#[command(version)]
struct Cli {
    /// Path to the configuration file.
    #[arg(short, long, default_value = "/etc/envproxy/config.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = Config::from_file(&cli.config).context("failed to load configuration")?;

    // Initialize tracing with the configured log level.
    let filter =
        EnvFilter::try_new(&config.agent.log_level).unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt().with_env_filter(filter).with_target(true).init();

    tracing::info!(
        config_path = %cli.config.display(),
        socket = %config.agent.socket.display(),
        "starting envproxy-agent"
    );

    // Create the backend based on configuration.
    let backend: Arc<dyn backend::Backend> = match &config.backend {
        BackendConfig::File { path } => {
            let file_backend = backend::file::FileBackend::new(path)
                .await
                .context("failed to initialize file backend")?;
            Arc::new(file_backend)
        }
        BackendConfig::Http(http_config) => {
            let http_backend = backend::http::HttpBackend::new(http_config.clone());
            Arc::new(http_backend)
        }
    };

    // Start the Unix socket server.
    let server = server::Server::bind(&config.agent.socket, backend)
        .await
        .context("failed to bind Unix socket")?;

    // Install signal handler for graceful shutdown.
    let shutdown = async {
        tokio::signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
        tracing::info!("received shutdown signal");
    };

    tokio::select! {
        result = server.run() => {
            result.context("server error")?;
        }
        () = shutdown => {
            tracing::info!("shutting down");
            // Clean up socket file.
            let _ = tokio::fs::remove_file(&config.agent.socket).await;
        }
    }

    Ok(())
}
