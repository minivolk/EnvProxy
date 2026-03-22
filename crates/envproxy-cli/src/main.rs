//! `envproxy` CLI — user-facing tool for running commands with envproxy interception.
//!
//! # Usage
//!
//! ```bash
//! # Run a command with envproxy interception
//! envproxy run -- python3 app.py
//!
//! # Test resolving a key via the agent
//! envproxy get DATABASE_URL
//!
//! # Check if the agent is running
//! envproxy status
//! ```

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use envproxy_proto::{decode_response, encode_request, Status, DEFAULT_SOCKET_PATH};

/// envproxy: transparent dynamic environment variable resolution.
#[derive(Parser)]
#[command(name = "envproxy")]
#[command(about = "Run commands with dynamic environment variable resolution via LD_PRELOAD")]
#[command(version)]
struct Cli {
    /// Path to the agent Unix socket.
    #[arg(short, long, default_value = DEFAULT_SOCKET_PATH, global = true)]
    socket: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a command with envproxy `LD_PRELOAD` interception.
    Run {
        /// The command and arguments to run.
        #[arg(trailing_var_arg = true, required = true)]
        cmd: Vec<String>,
    },

    /// Resolve a single key via the agent (for testing).
    Get {
        /// The environment variable key to resolve.
        key: String,
    },

    /// Check if the agent is running and responsive.
    Status,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { cmd } => cmd_run(&cmd, &cli.socket),
        Commands::Get { key } => cmd_get(&key, &cli.socket),
        Commands::Status => {
            cmd_status(&cli.socket);
            Ok(())
        }
    }
}

/// Run a command with `LD_PRELOAD` set to the envproxy shared library.
fn cmd_run(cmd: &[String], socket: &Path) -> Result<()> {
    if cmd.is_empty() {
        bail!("no command specified");
    }

    // Find the libenvproxy.so — check common locations.
    let lib_path = find_libenvproxy()?;

    let status = Command::new(&cmd[0])
        .args(&cmd[1..])
        .env("LD_PRELOAD", &lib_path)
        .env("ENVPROXY_SOCKET", socket.to_string_lossy().as_ref())
        .status()
        .with_context(|| format!("failed to execute: {}", cmd[0]))?;

    std::process::exit(status.code().unwrap_or(1));
}

/// Resolve a single key by connecting to the agent.
fn cmd_get(key: &str, socket: &Path) -> Result<()> {
    let response = query_agent(key.as_bytes(), socket)?;

    match response.status {
        Status::Found => {
            let value = String::from_utf8_lossy(&response.value);
            println!("{value}");
        }
        Status::NotFound => {
            eprintln!("key not found: {key}");
            std::process::exit(1);
        }
        Status::Error => {
            let msg = String::from_utf8_lossy(&response.value);
            eprintln!("error: {msg}");
            std::process::exit(1);
        }
        Status::Passthrough => {
            eprintln!("passthrough: key {key} is not intercepted");
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Check if the agent is running and responsive.
fn cmd_status(socket: &Path) {
    // Try to connect and send a test request.
    match query_agent(b"__ENVPROXY_PING__", socket) {
        Ok(_) => {
            println!("agent is running at {}", socket.display());
        }
        Err(e) => {
            eprintln!("agent is NOT running at {}: {e}", socket.display());
            std::process::exit(1);
        }
    }
}

/// Query the agent for a key via Unix socket.
fn query_agent(key: &[u8], socket: &Path) -> Result<envproxy_proto::DecodedResponse> {
    let encoded = encode_request(key).context("key too long")?;

    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("failed to connect to agent at {}", socket.display()))?;

    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    stream.write_all(&encoded).context("failed to send request")?;

    let mut buf = vec![0u8; 3 + envproxy_proto::MAX_VALUE_LEN];
    let n = stream.read(&mut buf).context("failed to read response")?;

    decode_response(&buf[..n]).map_err(|e| anyhow::anyhow!("decode error: {e}"))
}

/// Find the `libenvproxy.so` shared library.
///
/// Searches in common locations relative to the CLI binary.
fn find_libenvproxy() -> Result<String> {
    // Check for explicit environment variable.
    if let Ok(path) = std::env::var("ENVPROXY_LIB") {
        return Ok(path);
    }

    // Check relative to the current binary.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // In a cargo build, the .so is in the same target directory.
            let candidates = [dir.join("libenvproxy.so"), dir.join("../lib/libenvproxy.so")];
            for candidate in &candidates {
                if candidate.exists() {
                    return Ok(candidate.to_string_lossy().into_owned());
                }
            }
        }
    }

    // Common system paths.
    let system_paths = ["/usr/local/lib/libenvproxy.so", "/usr/lib/libenvproxy.so"];
    for path in &system_paths {
        if Path::new(path).exists() {
            return Ok((*path).to_owned());
        }
    }

    bail!(
        "libenvproxy.so not found. Set ENVPROXY_LIB to its path, or build it with:\n\
         cargo build --release -p libenvproxy"
    )
}
