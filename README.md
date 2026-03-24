# EnvProxy

Transparent dynamic environment variable resolution from remote secret sources — without modifying application code.

EnvProxy intercepts `getenv()` calls at the libc level via `LD_PRELOAD` and resolves them from a local agent daemon that fetches secrets from configured backends (file, HTTP API, Kubernetes Secrets, etc.). Secrets are fetched lazily, rotated dynamically, and never appear in `/proc/PID/environ`.

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
             │ Unix Socket
             ▼
┌──────────────────────────────────────────────────┐
│  envproxy-agent                                  │
│  ┌─────────┐  ┌────────────────┐                 │
│  │ mtime   │  │ Backend Plugin │                 │
│  │ reload  │  │ ┌────────────┐ │                 │
│  │         │  │ │ JSON file  │ │                 │
│  └─────────┘  │ ├────────────┤ │                 │
│               │ │ K8s Secret │ │                 │
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
- **Java-aware**: Automatically patches `System.getenv()` via a javaagent that replaces the internal `ProcessEnvironment` map.
- **Kubernetes-native**: Helm chart with DaemonSet agent + mutating webhook for automatic pod injection.
- **Fast**: Binary wire protocol over Unix socket. File backend checks mtime (one `stat` syscall) per request and only reloads on changes.

## Quick Start

### 1. Build

```bash
cargo build --release
```

This produces three artifacts in `target/release/`:

- `libenvproxy.so` — the LD_PRELOAD shared library
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
```

Your application's `getenv("DATABASE_URL")` calls will now be resolved from the agent.

## Dynamic Secret Rotation

envproxy supports live secret rotation without restarting any processes:

1. **Agent level**: The file backend checks the file's modification time on every request. When the file changes, it's automatically reloaded. The Kubernetes backend watches for Secret changes via the K8s API.

2. **Python/Java level**: The `os.environ` / `System.getenv()` proxies cache resolved values with a configurable TTL (default: 30 seconds). After expiry, the next lookup re-queries the agent.

```bash
# Set cache TTL to 5 seconds for faster rotation detection:
ENVPROXY_CACHE_TTL=5 envproxy run -- python3 app.py

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

# Kubernetes backend — reads from a K8s Secret (requires --features kubernetes)
# [backend]
# type = "kubernetes"
# namespace = "default"
# secret_name = "app-secrets"
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ENVPROXY_SOCKET` | `/tmp/envproxy/agent.sock` | Path to the agent Unix socket |
| `ENVPROXY_ENABLED` | `1` | Set to `0` to disable interception entirely |
| `ENVPROXY_DEBUG` | `0` | Set to `1` to enable debug output to stderr |
| `ENVPROXY_LIB` | *(auto-detected)* | Explicit path to `libenvproxy.so` for the CLI |
| `ENVPROXY_PYTHON_PATH` | *(auto-detected)* | Path to the Python support directory |
| `ENVPROXY_NO_PYTHON` | `0` | Set to `1` to disable Python `os.environ` patching |
| `ENVPROXY_JAVA_PATH` | *(auto-detected)* | Path to the Java support directory |
| `ENVPROXY_NO_JAVA` | `0` | Set to `1` to disable Java `System.getenv()` patching |
| `ENVPROXY_CACHE_TTL` | `30` | Python/Java cache TTL in seconds (0 = no caching) |

## Language Support

| Language | Mechanism | Dynamic Rotation |
|----------|-----------|-----------------|
| **C / C++** | `LD_PRELOAD` overrides `getenv()` | Every call hits agent (no caching) |
| **Python** | `sitecustomize.py` patches `os.environ` | TTL-based (default 30s) |
| **Node.js** | `LD_PRELOAD` — Node calls `getenv()` on every `process.env` access | Every access is live |
| **Ruby** | `LD_PRELOAD` — Ruby calls `getenv()` on every `ENV[]` access | Every access is live |
| **Java** | javaagent patches `ProcessEnvironment` via `JAVA_TOOL_OPTIONS` (set automatically) | TTL-based (default 30s) |
| **Go** | Does not use libc `getenv()` — requires companion package (planned) | N/A |

## Kubernetes

EnvProxy includes a Helm chart for Kubernetes deployments:

- **DaemonSet**: `envproxy-agent` runs on every node, exposes a Unix socket via `hostPath`
- **Mutating Webhook**: Automatically injects an init container and wraps pod entrypoints with `envproxy run --`
- **Kubernetes Secret backend**: Agent reads secrets from K8s Secrets with automatic reload via watch API

```bash
# Install
helm install envproxy k8s/chart/envproxy -n envproxy-system --create-namespace

# Label namespace for injection
kubectl label ns default envproxy.dev/injection=enabled

# Deploy a pod with envproxy injection
kubectl apply -f k8s/examples/python-app.yaml
```

See `k8s/chart/envproxy/values.yaml` for all configuration options.

## Project Structure

```
envproxy/
├── Cargo.toml                          # Workspace root
├── mise.toml                           # Dev tools + tasks
├── crates/
│   ├── envproxy-proto/                 # Wire protocol (zero dependencies)
│   ├── libenvproxy/                    # LD_PRELOAD .so (cdylib)
│   ├── envproxy-agent/                 # Local daemon (tokio async)
│   └── envproxy-cli/                   # CLI tool
├── support/
│   ├── python/                         # Python runtime hook
│   │   ├── sitecustomize.py            # Auto-loader with chaining
│   │   └── _envproxy_hook.py           # os.environ proxy
│   └── java/                           # Java runtime hook
│       ├── src/envproxy/               # EnvProxyAgent + EnvProxyMap
│       └── build.sh                    # Builds envproxy-agent.jar
├── k8s/
│   ├── Dockerfile                      # envproxy container image
│   ├── injector/                       # Go mutating webhook server
│   ├── chart/envproxy/                 # Helm chart
│   └── examples/                       # K8s example manifests
└── examples/
    ├── config.toml                     # Shared agent config
    ├── secrets.json                    # Shared example secrets
    ├── python/                         # Python demos + README
    ├── java/                           # Java demos + README
    ├── node/                           # Node.js demos + README
    └── c/                              # C demos + README
```

## Security Model

- Secrets are fetched over a **Unix socket** (local only, no network exposure).
- Secrets are **never written to the process environment** — they exist only in the agent's memory and the application's heap.
- `/proc/PID/environ` does **not** contain secrets.
- The agent socket can be protected with filesystem permissions.
- `ENVPROXY_` prefixed variables are never intercepted (prevents recursion and config leaks).

## Comparison with Existing Tools

| Feature | envproxy | envconsul | bank-vaults | dotenv |
|---------|----------|-----------|-------------|--------|
| Dynamic rotation (no restart) | Yes | No (sets at start) | No (mutating webhook) | No |
| Lazy fetching | Yes | No (fetches all) | No (fetches all) | No |
| Secrets in `/proc/environ` | No | Yes | Yes | Yes |
| Language-agnostic | Yes | Yes | Kubernetes only | Per-language |
| No code changes | Yes | Yes | Yes | No |
| Works outside Kubernetes | Yes | Yes | No | Yes |

## Building from Source

```bash
# Clone the repository
git clone https://github.com/minivolk/EnvProxy.git
cd EnvProxy

# Build all crates
cargo build --release

# Build with Kubernetes backend support
cargo build --release --features kubernetes

# Build Java agent JAR
mise run build:java

# Run tests
cargo test

# Run clippy
cargo clippy --all-targets --all-features -- -D warnings
```

### Requirements

- Rust 1.85+ (2021 edition)
- Linux (`LD_PRELOAD` is Linux/Unix-specific)
- Python 3.8+ (for the `os.environ` proxy)
- Java 16+ (for Unix domain socket support in the Java agent)
- Go 1.23+ (for the Kubernetes webhook injector)

## License

Licensed under either of:

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
