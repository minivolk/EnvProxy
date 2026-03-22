# EnvProxy

Transparent dynamic environment variable resolution from remote secret sources — without modifying application code.

envproxy intercepts `getenv()` calls at the libc level via `LD_PRELOAD` and resolves them from a local agent daemon that fetches secrets from configured backends (file, HTTP API, Vault, etc.). Secrets are fetched lazily, rotated dynamically, and never appear in `/proc/PID/environ`.

## How It Works

```
┌──────────────────────────────────────────────────┐
│  Application Process (Python, C, Node.js, etc.)  │
│                                                  │
│   app calls getenv("DATABASE_URL")               │
│          │                                       │
│          ▼                                       │
│   ┌──────────────────┐                           │
│   │ libenvproxy.so   │  (LD_PRELOAD)             │
│   │                  │                           │
│   │ 1. Check key     │                           │
│   │ 2. Query agent   │                           │
│   │    via Unix sock  │                          │
│   └────────┬─────────┘                           │
└────────────┼─────────────────────────────────────┘
             │ Unix Socket (/tmp/envproxy/agent.sock)
             ▼
┌──────────────────────────────────────────────────┐
│  envproxy-agent                                  │
│  ┌─────────┐  ┌────────────────┐                 │
│  │ mtime   │  │ Backend Plugin │                 │
│  │ reload  │  │ ┌────────────┐ │                 │
│  │         │  │ │ JSON file  │ │                 │
│  └─────────┘  │ ├────────────┤ │                 │
│               │ │ HTTP API   │ │                 │
│               │ └────────────┘ │                 │
│               └────────────────┘                 │
└──────────────────────────────────────────────────┘
```

## Key Features

- **Transparent**: Works with any dynamically-linked binary — Python, Ruby, Node.js, C, C++, Java, etc. No code changes required.
- **Dynamic**: Secrets are fetched at call time. Rotate secrets by updating the source — running processes pick up new values automatically.
- **Lazy**: Only secrets that are actually requested are fetched. No bulk loading at startup.
- **Secure**: Secrets never appear in `/proc/PID/environ` or `ps e` output.
- **Python-aware**: Automatically patches `os.environ` via `sitecustomize.py` so `os.getenv()` works transparently, with configurable cache TTL.
- **Fast**: Binary wire protocol over Unix socket. File backend checks mtime (one `stat` syscall) per request and only reloads on changes.

## Quick Start

### 1. Build

```bash
cargo build --release
```

This produces three artifacts in `target/release/`:

- `libenvproxy.so` — the LD_PRELOAD shared library (418 KB)
- `envproxy-agent` — the local daemon
- `envproxy` — the CLI tool

### 2. Create a secrets file

```json
{
  "DATABASE_URL": "postgres://user:secret@localhost:5432/mydb",
  "API_KEY": "sk-1234567890",
  "REDIS_URL": "redis://localhost:6379/0"
}
```

### 3. Create an agent config

```toml
# config.toml
[agent]
socket = "/tmp/envproxy/agent.sock"
log_level = "info"

[backend]
type = "file"
path = "/path/to/secrets.json"
```

### 4. Start the agent

```bash
envproxy-agent --config config.toml
```

### 5. Run your application

```bash
# Using the CLI wrapper (sets LD_PRELOAD automatically):
envproxy run -- python3 app.py

# Or manually:
LD_PRELOAD=/path/to/libenvproxy.so python3 app.py

# For Python with full os.getenv() support:
ENVPROXY_PYTHON_PATH=/path/to/envproxy/python \
LD_PRELOAD=/path/to/libenvproxy.so \
python3 app.py
```

Your application's `getenv("DATABASE_URL")` calls will now be resolved from the agent.

## Dynamic Secret Rotation

envproxy supports live secret rotation without restarting any processes:

1. **Agent level**: The file backend checks the file's modification time on every request. When the file changes, it's automatically reloaded.

2. **Python level**: The `os.environ` proxy caches resolved values with a configurable TTL (default: 30 seconds). After expiry, the next `os.getenv()` re-queries the agent.

```bash
# Set cache TTL to 5 seconds for faster rotation detection:
ENVPROXY_CACHE_TTL=5 \
ENVPROXY_PYTHON_PATH=/path/to/envproxy/python \
LD_PRELOAD=/path/to/libenvproxy.so \
python3 app.py

# Now edit your secrets file — the running process will pick up
# new values within 5 seconds, no restart needed.
```

## CLI Reference

```bash
# Check if the agent is running
envproxy status

# Resolve a single key (useful for testing)
envproxy get DATABASE_URL

# Run a command with envproxy interception
envproxy run -- python3 app.py
envproxy run -- node server.js
envproxy run -- ./my-c-program
```

## Configuration

### Agent Config (`config.toml`)

```toml
[agent]
socket = "/tmp/envproxy/agent.sock"   # Unix socket path
log_level = "info"                     # trace, debug, info, warn, error

# File backend — reads secrets from a JSON file
[backend]
type = "file"
path = "/etc/envproxy/secrets.json"

# HTTP backend (placeholder — not yet fully implemented)
# [backend]
# type = "http"
# url = "https://secrets.internal/v1/env"
# auth_token = "my-token"
```

### Environment Variables

| Variable               | Default                    | Description                                        |
| ---------------------- | -------------------------- | -------------------------------------------------- |
| `ENVPROXY_SOCKET`      | `/tmp/envproxy/agent.sock` | Path to the agent Unix socket                      |
| `ENVPROXY_ENABLED`     | `1`                        | Set to `0` to disable interception entirely        |
| `ENVPROXY_DEBUG`       | `0`                        | Set to `1` to enable debug output to stderr        |
| `ENVPROXY_PYTHON_PATH` | _(auto-detected)_          | Path to the Python support directory               |
| `ENVPROXY_NO_PYTHON`   | `0`                        | Set to `1` to disable Python `os.environ` patching |
| `ENVPROXY_CACHE_TTL`   | `30`                       | Python cache TTL in seconds (0 = no caching)       |
| `ENVPROXY_LIB`         | _(auto-detected)_          | Explicit path to `libenvproxy.so` for the CLI      |

## Language Support

| Language    | Mechanism                                                           | Dynamic Rotation                   |
| ----------- | ------------------------------------------------------------------- | ---------------------------------- |
| **C / C++** | `LD_PRELOAD` overrides `getenv()`                                   | Every call hits agent (no caching) |
| **Python**  | `sitecustomize.py` patches `os.environ`                             | TTL-based (default 30s)            |
| **Node.js** | `LD_PRELOAD` — Node calls `getenv()` on every `process.env` access  | Every access is live               |
| **Ruby**    | `LD_PRELOAD` — Ruby calls `getenv()` on every `ENV[]` access        | Every access is live               |
| **Java**    | `LD_PRELOAD` — JNI calls `getenv()` for `System.getenv()`           | Every call is live                 |
| **Go**      | Does not use libc `getenv()` — requires companion package (planned) | N/A                                |

## Project Structure

```
envproxy/
├── Cargo.toml                          # Workspace root
├── crates/
│   ├── envproxy-proto/                 # Wire protocol (zero dependencies)
│   ├── libenvproxy/                    # LD_PRELOAD .so (cdylib)
│   ├── envproxy-agent/                 # Local daemon (tokio async)
│   └── envproxy-cli/                   # CLI tool
├── python/
│   ├── _envproxy_hook.py               # os.environ proxy
│   ├── sitecustomize.py                # Auto-loader with chaining
│   └── _envproxy.pth                   # For site-packages installs
└── examples/
    ├── config.toml                     # Example agent config
    ├── secrets.json                    # Example secrets file
    └── python_demo.py                  # Python demo script
```

## Security Model

- Secrets are fetched over a **Unix socket** (local only, no network exposure).
- Secrets are **never written to the process environment** — they exist only in the agent's memory and the application's heap.
- `/proc/PID/environ` does **not** contain secrets (verified by tests).
- The agent socket can be protected with filesystem permissions.
- `ENVPROXY_` prefixed variables are never intercepted (prevents recursion and config leaks).

## Comparison with Existing Tools

| Feature                       | envproxy | envconsul          | bank-vaults           | dotenv       |
| ----------------------------- | -------- | ------------------ | --------------------- | ------------ |
| Dynamic rotation (no restart) | Yes      | No (sets at start) | No (mutating webhook) | No           |
| Lazy fetching                 | Yes      | No (fetches all)   | No (fetches all)      | No           |
| Secrets in `/proc/environ`    | No       | Yes                | Yes                   | Yes          |
| Language-agnostic             | Yes      | Yes                | Kubernetes only       | Per-language |
| No code changes               | Yes      | Yes                | Yes                   | No           |
| Works outside Kubernetes      | Yes      | Yes                | No                    | Yes          |

## Building from Source

```bash
# Clone the repository
git clone https://github.com/minivolk/envproxy.git
cd envproxy

# Build all crates
cargo build --release

# Run tests
cargo test

# Run clippy
cargo clippy --all-targets --all-features -- -D warnings
```

### Requirements

- Rust 1.75+ (2021 edition)
- Linux (LD_PRELOAD is Linux/Unix-specific)
- Python 3.8+ (for the Python `os.environ` proxy)

## License

Licensed under either of:

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
