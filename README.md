# EnvProxy

[![CI](https://github.com/minivolk/EnvProxy/actions/workflows/ci.yml/badge.svg)](https://github.com/minivolk/EnvProxy/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/minivolk/EnvProxy)](https://github.com/minivolk/EnvProxy/releases)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange)](https://www.rust-lang.org/)
[![Go](https://img.shields.io/badge/go-1.26%2B-00ADD8)](https://go.dev/)
[![Docker](https://img.shields.io/badge/docker-ghcr.io%2Fminivolk%2Fenvproxy-blue)](https://ghcr.io/minivolk/envproxy)
[![Helm](https://img.shields.io/badge/helm-oci%3A%2F%2Fghcr.io%2Fminivolk%2Fcharts%2Fenvproxy-blue)](https://ghcr.io/minivolk/charts/envproxy)
[![Vault Compatible](https://img.shields.io/badge/vault-compatible-black?logo=vault)](https://www.vaultproject.io/)
[![Platform](https://img.shields.io/badge/platform-linux-lightgrey)](https://kernel.org/)

Transparent dynamic environment variable resolution from remote secret sources — without modifying application code.

EnvProxy intercepts `getenv()` calls at the libc level via `LD_PRELOAD` and resolves them from a sidecar agent that fetches secrets from configured backends (HashiCorp Vault, file, HTTP API). Secrets are fetched lazily, rotated dynamically, and never appear in `/proc/PID/environ`.

## How It Works

### Local (standalone)

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
│  │ Cache   │  │ Backend Plugin │                 │
│  │ (TTL)   │  │ ┌────────────┐ │                 │
│  │         │  │ │ JSON file  │ │                 │
│  └─────────┘  │ ├────────────┤ │                 │
│               │ │ Vault      │ │                 │
│               │ └────────────┘ │                 │
│               └────────────────┘                 │
└──────────────────────────────────────────────────┘
```

### Kubernetes (sidecar + Vault)

```
┌──────────────────────────────────────────────────────┐
│  Application Pod                                      │
│                                                      │
│  ┌─────────────┐  ┌──────────────────────────────┐   │
│  │ init:        │  │ sidecar: envproxy-agent      │   │
│  │ envproxy-init│  │                              │   │
│  │ copies bins  │  │ - K8s auth → Vault           │   │
│  │ + config     │  │ - Resolves vault:path#key    │   │
│  └──────┬──────┘  │ - Cache + TTL + token renew   │   │
│         │         │ - Unix socket /envproxy/sock   │   │
│         ▼         └──────────────┬───────────────┘   │
│  ┌──────────────┐               │                    │
│  │ app container │◄──────────────┘                    │
│  │               │  Unix socket                      │
│  │ env:          │                                    │
│  │  DATABASE_URL=│vault:secret/data/myapp/cfg#db_url │
│  │               │                                    │
│  │ LD_PRELOAD    │                                    │
│  │ getenv() ─────┼──► agent ──► Vault ──► value      │
│  └──────────────┘                                    │
│                                                      │
│  volumes: envproxy-bin (emptyDir, shared)             │
└──────────────────────────────────────────────────────┘
```

## Key Features

- **Transparent**: Works with any dynamically-linked binary — Python, Ruby, Node.js, C, C++, Java. No code changes required.
- **Dynamic**: Secrets are fetched at call time. Rotate secrets in Vault — running processes pick up new values automatically via TTL-based cache refresh.
- **Vault-native**: Bank-vaults style `vault:path#key` syntax in env vars. Kubernetes auth with per-pod service accounts. Automatic token renewal.
- **Lazy**: Only secrets that are actually requested are fetched. No bulk loading at startup.
- **Secure**: Secrets never appear in `/proc/PID/environ` — the process environment shows `vault:path#key`, not the real secret value.
- **Python-aware**: Automatically patches `os.environ` via `sitecustomize.py` so `os.getenv()` resolves `vault:` references transparently.
- **Java-aware**: Automatically patches `System.getenv()` via a javaagent that replaces the internal `ProcessEnvironment` map.
- **Kubernetes-native**: Helm chart with mutating webhook. Injects sidecar agent + init container automatically. Pod annotations for Vault configuration.
- **Fast**: Binary wire protocol (v2) over Unix socket. Vault responses are cached per-path with configurable TTL.

## Use Cases

### Database Password Rotation Without Restarts

Your DBA rotates the database password every 24 hours via Vault. With static env vars, you need to restart every pod to pick up the new password — coordinating rolling restarts across dozens of services during the rotation window. With EnvProxy, the next time your connection pool calls `getenv("DATABASE_URL")` to create a new connection, it gets the new password automatically. No restart, no coordination, no downtime.

### Long-Running Processes

You have a data pipeline that runs for hours or days — batch jobs, ML training, stream processors. Static env vars mean the process uses whatever credentials it started with. If those credentials expire or are revoked mid-run, the process fails. With EnvProxy, the process always gets current, valid credentials on each `getenv()` call.

### Hot-Reloading API Keys

Your application calls a third-party API with a key stored in Vault. The vendor rotates the key — maybe you hit a rate limit and need to switch to a backup key, or the key was compromised and needs immediate replacement. With static env vars, you'd need to redeploy. With EnvProxy, update the value in Vault and every running instance picks it up within the cache TTL — seconds, not minutes.

### Feature Flags via Environment

Your platform uses environment variables for feature flags or configuration toggles (`FEATURE_NEW_CHECKOUT=true`). With static env vars, toggling a flag requires a redeploy. With EnvProxy backed by a file or Vault, you update the source and all running processes see the new value on the next `getenv()` call — instant rollout, instant rollback.

### Secret Leak Prevention

With envconsul or bank-vaults, after secrets are injected at startup, `cat /proc/1/environ` reveals every secret in plaintext. An attacker with `kubectl exec` access sees everything. With EnvProxy, `/proc/environ` only shows `vault:secret/data/myapp/config#DATABASE_URL` — the reference, never the real value. The secret only exists in the agent's memory and the application's heap, never in the kernel's process environment block.

### Zero-Code Vault Migration

You have 50 microservices in Python, Java, and Node.js. Each would need a Vault client library, connection setup, error handling, and caching logic — different for each language. With EnvProxy, you change one line per env var (`value: "vault:secret/data/..."`) and add one annotation (`envproxy.dev/inject: "true"`). The application code stays exactly the same — `os.getenv("DATABASE_URL")` still works, it just resolves from Vault now instead of a static string.

## Quick Start

### Local Development (file backend)

```bash
# 1. Build
cargo build --release

# 2. Create a secrets file
cat > secrets.json << 'EOF'
{
  "DATABASE_URL": "postgres://user:secret@localhost:5432/mydb",
  "API_KEY": "sk-1234567890"
}
EOF

# 3. Create agent config
cat > config.toml << 'EOF'
[agent]
socket = "/tmp/envproxy/agent.sock"
log_level = "info"

[backend]
type = "file"
path = "secrets.json"
EOF

# 4. Start the agent
envproxy-agent --config config.toml &

# 5. Run your application
envproxy run -- python3 app.py
```

### Kubernetes with Vault

```bash
# 1. Install envproxy from OCI registry
helm install envproxy oci://ghcr.io/minivolk/charts/envproxy \
  -n envproxy-system --create-namespace

# 2. Label namespace for injection
kubectl label ns default envproxy.dev/injection=enabled

# 3. Deploy a pod with vault: env vars
kubectl apply -f - << 'EOF'
apiVersion: v1
kind: Pod
metadata:
  name: myapp
  annotations:
    envproxy.dev/inject: "true"
    envproxy.dev/vault-addr: "https://vault.internal:8200"
    envproxy.dev/vault-role: "myapp"
spec:
  serviceAccountName: myapp
  containers:
    - name: app
      image: python:3.12-slim
      command: ["python3", "app.py"]
      env:
        - name: DATABASE_URL
          value: "vault:secret/data/myapp/config#DATABASE_URL"
        - name: API_KEY
          value: "vault:secret/data/myapp/config#API_KEY"
EOF
```

The mutating webhook automatically injects the sidecar agent, init container, and wraps the entrypoint. Your app calls `os.getenv("DATABASE_URL")` and gets the real secret value from Vault.

## Vault Integration

### `vault:` Prefix Syntax

Environment variable values starting with `vault:` are resolved from HashiCorp Vault at runtime:

```
vault:<mount>/data/<path>#<key>
vault:<mount>/data/<path>#<key>#<version>
```

Examples:

| Env Var Value | Vault Path | Key | Version |
|---------------|------------|-----|---------|
| `vault:secret/data/myapp/config#DATABASE_URL` | `secret/myapp/config` | `DATABASE_URL` | latest |
| `vault:secret/data/myapp/db#password#3` | `secret/myapp/db` | `password` | 3 |
| `vault:kv/data/team/prod/api#token` | `kv/team/prod/api` | `token` | latest |

Non-prefixed values pass through as-is:

```yaml
env:
  - name: DATABASE_URL
    value: "vault:secret/data/myapp/config#DATABASE_URL"  # resolved from Vault
  - name: LOG_LEVEL
    value: "info"                                          # passed through unchanged
```

### Pod Annotations

| Annotation | Default | Description |
|------------|---------|-------------|
| `envproxy.dev/inject` | *(required)* | Set to `"true"` to enable injection |
| `envproxy.dev/vault-addr` | *(required)* | Vault server address (e.g., `https://vault.internal:8200`) |
| `envproxy.dev/vault-role` | *(required)* | Vault auth role name |
| `envproxy.dev/vault-auth-method` | `kubernetes` | Auth method: `kubernetes`, `token` |
| `envproxy.dev/vault-auth-mount` | `kubernetes` | Vault auth mount path (e.g., `kubernetes_my-cluster`) |
| `envproxy.dev/vault-cache-ttl` | `5m` | Cache TTL for resolved secrets |
| `envproxy.dev/cache-ttl` | `30` | Python/Java proxy cache TTL (seconds) |
| `envproxy.dev/containers` | *(all)* | Comma-separated list of containers to inject |
| `envproxy.dev/no-python` | `false` | Disable Python `os.environ` patching |
| `envproxy.dev/no-java` | `false` | Disable Java `System.getenv()` patching |

### How It Works in Kubernetes

1. **Webhook** intercepts pod creation, sees `envproxy.dev/inject: "true"`
2. **Init container** copies envproxy binaries + generates `config.toml` from annotations into a shared emptyDir volume
3. **Sidecar container** starts `envproxy-agent` with Vault backend, authenticates using the pod's service account, listens on Unix socket
4. **App container** entrypoint is wrapped with `envproxy run --`, which waits for the agent socket, then `exec()`'s into the original command with `LD_PRELOAD` set
5. **At runtime**, `getenv("DATABASE_URL")` is intercepted, the `vault:` prefixed value is sent to the agent (v2 protocol), the agent fetches from Vault, caches the result, and returns the secret

### Vault Auth Setup

```bash
# Enable Kubernetes auth in Vault
vault auth enable kubernetes
# or with a custom mount path:
vault auth enable -path=kubernetes_my-cluster kubernetes

# Configure the auth method
vault write auth/kubernetes/config \
  kubernetes_host="https://kubernetes.default.svc"

# Create a policy
vault policy write myapp-policy - << 'EOF'
path "secret/data/myapp/*" {
  capabilities = ["read"]
}
EOF

# Create a role bound to a service account
vault write auth/kubernetes/role/myapp \
  bound_service_account_names=myapp \
  bound_service_account_namespaces=default \
  policies=myapp-policy \
  ttl=1h
```

### Dynamic Secret Rotation

Secrets are dynamically resolved with TTL-based caching:

1. **First call**: `getenv("DATABASE_URL")` → agent fetches from Vault → caches with TTL
2. **Subsequent calls**: served from cache (no Vault request)
3. **After TTL expires**: next call re-fetches from Vault → picks up rotated value
4. **Vault token**: automatically renewed at 2/3 of lease duration

```yaml
annotations:
  envproxy.dev/vault-cache-ttl: "30s"  # re-fetch from Vault every 30 seconds
```

## Configuration

### Agent Config (`config.toml`)

```toml
[agent]
socket = "/tmp/envproxy/agent.sock"
log_level = "info"

# File backend — reads secrets from a JSON file
[backend]
type = "file"
path = "/etc/envproxy/secrets.json"

# Vault backend — resolves vault: prefixed env vars (requires --features vault)
# [backend]
# type = "vault"
# address = "https://vault.internal:8200"
# auth_method = "kubernetes"
# auth_mount = "kubernetes"
# role = "myapp"
# cache_ttl = "5m"
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

| Language | Mechanism | `vault:` Support |
|----------|-----------|-----------------|
| **C / C++** | `LD_PRELOAD` overrides `getenv()` | Yes — `.so` sends v2 protocol with vault: value |
| **Python** | `sitecustomize.py` patches `os.environ` | Yes — proxy detects vault: prefix, sends v2 |
| **Node.js** | `LD_PRELOAD` — Node calls `getenv()` on every `process.env` access | Yes — via `.so` v2 protocol |
| **Ruby** | `LD_PRELOAD` — Ruby calls `getenv()` on every `ENV[]` access | Yes — via `.so` v2 protocol |
| **Java** | javaagent patches `ProcessEnvironment` map | Yes — proxy detects vault: prefix, sends v2 |
| **Go** | Does not use libc `getenv()` — requires companion package (planned) | N/A |

## CLI Reference

```bash
# Run a command with envproxy interception
envproxy run -- python3 app.py
envproxy run -- node server.js

# Check if the agent is running
envproxy status

# Resolve a single key (useful for testing)
envproxy get DATABASE_URL
```

## Project Structure

```
envproxy/
├── Cargo.toml                          # Workspace root
├── mise.toml                           # Dev tools + tasks
├── crates/
│   ├── envproxy-proto/                 # Wire protocol v1/v2 + vault: ref parser
│   ├── libenvproxy/                    # LD_PRELOAD .so (cdylib)
│   ├── envproxy-agent/                 # Sidecar agent (tokio async)
│   └── envproxy-cli/                   # CLI tool
├── support/
│   ├── python/                         # Python runtime hook
│   │   ├── sitecustomize.py            # Auto-loader with chaining
│   │   └── _envproxy_hook.py           # os.environ proxy (v2 protocol)
│   └── java/                           # Java runtime hook
│       ├── src/envproxy/               # EnvProxyAgent + EnvProxyMap (v2 protocol)
│       └── build.sh                    # Builds envproxy-agent.jar
├── k8s/
│   ├── Dockerfile                      # envproxy container image
│   ├── injector/                       # Go mutating webhook (sidecar injection)
│   ├── chart/envproxy/                 # Helm chart
│   └── examples/                       # K8s manifests (vault-app, policy)
└── examples/
    ├── config.toml                     # Shared agent config
    ├── secrets.json                    # Shared example secrets
    ├── python/                         # Python demos + README
    ├── java/                           # Java demos + README
    ├── node/                           # Node.js demos + README
    └── c/                              # C demos + README
```

## Security Model

- Secrets are fetched over a **Unix socket** (local only, no network exposure)
- `/proc/PID/environ` shows `vault:path#key` — **never the real secret value**
- Secrets exist only in the agent's cache and the application's heap
- Each pod authenticates to Vault with **its own service account** (per-pod Vault roles)
- Vault tokens are **automatically renewed** before expiry
- `ENVPROXY_` prefixed variables are never intercepted (prevents recursion)

## Comparison with Existing Tools

| Feature | envproxy | envconsul | bank-vaults | dotenv |
|---------|----------|-----------|-------------|--------|
| Dynamic rotation (no restart) | Yes (TTL cache) | No (sets at start) | No (resolves at start) | No |
| Lazy fetching | Yes | No (fetches all) | No (fetches all) | No |
| Secrets in `/proc/environ` | No (`vault:path#key`) | Yes | Yes (after resolve) | Yes |
| Language-agnostic | Yes | Yes | Yes | Per-language |
| No code changes | Yes | Yes | Yes | No |
| Per-pod Vault auth | Yes (sidecar) | No | Yes (webhook) | No |
| Works outside Kubernetes | Yes | Yes | No | Yes |
| Versioned secret reads | Yes (`#key#3`) | No | No | No |

## Building from Source

```bash
# Clone the repository
git clone https://github.com/minivolk/EnvProxy.git
cd EnvProxy

# Build all crates (local backends only)
cargo build --release

# Build with Vault support
cargo build --release --features vault

# Build with all backends (Vault + Kubernetes Secrets)
cargo build --release --features full

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
- Go 1.26+ (for the Kubernetes webhook injector)

## License

Licensed under either of:

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
