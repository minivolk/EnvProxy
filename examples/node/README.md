# Node.js Integration

## How It Works

Node.js calls libc's `getenv()` on **every** `process.env` access (via libuv). This means envproxy's `LD_PRELOAD` interception works **out of the box** with zero additional patching or configuration.

The flow:

```
LD_PRELOAD loads libenvproxy.so
  |
  +-- overrides getenv() in the dynamic linker
  |
  v
node starts
  |
  v
process.env.DATABASE_URL
  |
  +-- Node/libuv calls getenv("DATABASE_URL")
  +-- libenvproxy.so intercepts the call
  +-- queries envproxy-agent via Unix socket -> returns value
```

## Quick Start

```bash
# Terminal 1: start the agent
mise run agent

# Terminal 2: run the demo
mise run demo:node
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `ENVPROXY_ENABLED` | `1` | Set to `0` to disable envproxy entirely. |
| `ENVPROXY_DEBUG` | `0` | Set to `1` for debug output to stderr. |

## Key Properties

- **Zero configuration**: No agent JAR, no sitecustomize, no hooks. Just `LD_PRELOAD`.
- **Every access is live**: Node calls `getenv()` on each `process.env` read, so secret rotation is picked up immediately without any TTL.
- **No caching**: Values are resolved fresh on every access (the C-level `getenv` override queries the agent each time).

## Files

| File | Description |
|------|-------------|
| `demo.mjs` | Live monitoring loop — shows env vars refreshing every 3 seconds |
