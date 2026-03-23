# C / C++ Integration

## How It Works

C and C++ programs call libc's `getenv()` directly. This is the most fundamental integration — envproxy's `LD_PRELOAD` library overrides `getenv()` (and `secure_getenv()`) at the dynamic linker level, so every call is intercepted transparently.

The flow:

```
LD_PRELOAD loads libenvproxy.so
  |
  +-- overrides getenv() and secure_getenv()
  |
  v
Application calls getenv("DATABASE_URL")
  |
  +-- libenvproxy.so intercepts the call
  +-- queries envproxy-agent via Unix socket -> returns value
  +-- if agent is unavailable, falls back to the real getenv()
```

## Quick Start

```bash
# Build the demo
cc -o /tmp/envproxy/demo_c examples/c/demo.c

# Terminal 1: start the agent
mise run agent

# Terminal 2: run the demo
mise run demo:c
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `ENVPROXY_SOCKET` | `/tmp/envproxy/agent.sock` | Path to the agent Unix socket. |
| `ENVPROXY_ENABLED` | `1` | Set to `0` to disable envproxy entirely. |
| `ENVPROXY_DEBUG` | `0` | Set to `1` for debug output to stderr. |

## Key Properties

- **Zero configuration**: Just `LD_PRELOAD`. No hooks, no agents, no patching.
- **Every call is live**: Each `getenv()` call queries the agent, so secret rotation is immediate.
- **Graceful fallback**: If the agent socket is unavailable, the real `getenv()` is called (passthrough).
- **Works with C++**: `std::getenv()` calls libc's `getenv()`, so it's intercepted too.
- **Thread-safe**: The Unix socket communication is per-call with a 500ms timeout.

## Limitations

- **Statically-linked binaries** bypass `LD_PRELOAD` entirely. If your binary is linked with `-static`, envproxy cannot intercept it.
- **`ENVPROXY_*` variables** are never intercepted (to prevent recursion).

## Files

| File | Description |
|------|-------------|
| `demo.c` | Live monitoring loop — prints env vars every 3 seconds |
