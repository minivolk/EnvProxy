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
use std::os::unix::process::CommandExt;
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

    /// Copy envproxy runtime files to a target directory and optionally write agent config.
    /// Used by the Kubernetes init container (works on distroless images, no shell needed).
    Init {
        /// Target directory to copy files to.
        #[arg(short, long, default_value = "/envproxy")]
        target: PathBuf,

        /// Agent config content to write to <target>/config.toml.
        /// Passed as a string (the webhook generates it from pod annotations).
        #[arg(long)]
        write_config: Option<String>,
    },
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
        Commands::Init { target, write_config } => cmd_init(&target, write_config.as_deref()),
    }
}

/// Run a command with `LD_PRELOAD` set to the envproxy shared library.
///
/// Uses `exec()` to replace the current process with the target command.
/// This is the correct pattern for container entrypoint wrappers because:
/// - PID 1 receives signals directly (SIGTERM for graceful shutdown)
/// - `LD_PRELOAD` and `PYTHONPATH` are visible in PID 1's environment
/// - No zombie process issues from a wrapper parent
fn cmd_run(cmd: &[String], socket: &Path) -> Result<()> {
    if cmd.is_empty() {
        bail!("no command specified");
    }

    // Find the libenvproxy.so — check common locations.
    let lib_path = find_libenvproxy()?;

    let mut command = Command::new(&cmd[0]);
    command.args(&cmd[1..]);
    command.env("LD_PRELOAD", &lib_path);

    // Only set ENVPROXY_SOCKET if not already in the environment.
    // In Kubernetes, the webhook sets it to the correct hostPath value
    // (e.g., /envproxy/agent.sock); don't overwrite it with the
    // CLI's default (/tmp/envproxy/agent.sock).
    let socket_path = if let Ok(s) = std::env::var("ENVPROXY_SOCKET") {
        s
    } else {
        let s = socket.to_string_lossy().into_owned();
        command.env("ENVPROXY_SOCKET", &s);
        s
    };

    // Wait for the agent socket to be ready before exec'ing.
    // In the sidecar model, the agent and app containers start simultaneously.
    // The agent needs a moment to create the Unix socket. Without this wait,
    // the app starts before the socket exists, and the Python/Java hooks
    // skip installation because they check for socket existence at startup.
    if !socket_path.is_empty() {
        let max_wait = Duration::from_secs(10);
        let poll_interval = Duration::from_millis(100);
        let start = std::time::Instant::now();

        while !std::path::Path::new(&socket_path).exists() {
            if start.elapsed() >= max_wait {
                eprintln!(
                    "[envproxy] warning: agent socket {socket_path} not found after {max_wait:?}, proceeding anyway"
                );
                break;
            }
            std::thread::sleep(poll_interval);
        }
    }

    // Replace the current process with the target command.
    let err = command.exec();
    bail!("exec failed: {err}");
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

/// Copy envproxy runtime files to a target directory.
///
/// This replaces the `sh -c "cp ... && mkdir ..."` init container script,
/// enabling the use of distroless images (no shell required).
/// Optionally writes a config.toml file if `config_content` is provided.
fn cmd_init(target: &Path, config_content: Option<&str>) -> Result<()> {
    use std::fs;

    // Create directory structure.
    let dirs = ["lib", "python", "java"];
    for dir in &dirs {
        fs::create_dir_all(target.join(dir))
            .with_context(|| format!("failed to create {}/{dir}", target.display()))?;
    }

    // Source → destination mappings.
    let files: &[(&str, &str)] = &[
        ("/usr/bin/envproxy", "envproxy"),
        ("/usr/bin/envproxy-agent", "envproxy-agent"),
        ("/usr/lib/envproxy/lib/libenvproxy.so", "lib/libenvproxy.so"),
        ("/usr/lib/envproxy/java/envproxy-agent.jar", "java/envproxy-agent.jar"),
    ];

    for (src, dst) in files {
        let dest = target.join(dst);
        fs::copy(src, &dest)
            .with_context(|| format!("failed to copy {src} → {}", dest.display()))?;
    }

    // Copy Python support directory (multiple files).
    let python_src = Path::new("/usr/lib/envproxy/python");
    if python_src.is_dir() {
        for entry in fs::read_dir(python_src).context("failed to read python dir")? {
            let entry = entry?;
            let src_path = entry.path();
            if src_path.is_file() {
                let file_name = entry.file_name();
                let dest = target.join("python").join(&file_name);
                fs::copy(&src_path, &dest).with_context(|| {
                    format!("failed to copy {} → {}", src_path.display(), dest.display())
                })?;
            }
        }
    }

    // Set executable permissions.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let executables = ["envproxy", "envproxy-agent"];
        for name in &executables {
            let path = target.join(name);
            if path.exists() {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                    .with_context(|| format!("failed to chmod {}", path.display()))?;
            }
        }
    }

    // Write config.toml if provided.
    if let Some(content) = config_content {
        let config_path = target.join("config.toml");
        fs::write(&config_path, content)
            .with_context(|| format!("failed to write {}", config_path.display()))?;
    }

    println!("envproxy init: copied runtime files to {}", target.display());
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
