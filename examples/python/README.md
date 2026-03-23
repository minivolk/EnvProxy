# Python Integration

## How It Works

Python copies the process environment into an internal dict (`os.environ`) at startup and never calls libc's `getenv()` again. To intercept `os.getenv()`, envproxy uses a **`sitecustomize.py` hook** that replaces `os.environ` with a custom `MutableMapping` proxy.

The flow:

```
LD_PRELOAD loads libenvproxy.so
  |
  +-- .so constructor sets PYTHONPATH to include envproxy's python/ dir
  |
  v
Python starts -> site.py imports sitecustomize.py from PYTHONPATH
  |
  +-- sitecustomize.py imports _envproxy_hook
  +-- _envproxy_hook replaces os.environ with _EnvProxyEnviron proxy
  |
  v
os.getenv("DATABASE_URL")
  |
  +-- proxy checks real environ -> miss
  +-- queries envproxy-agent via Unix socket -> returns value
  +-- caches result with TTL (default: 30s)
```

## Quick Start

```bash
# Terminal 1: start the agent
mise run agent

# Terminal 2: run the demo
mise run demo:python
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `ENVPROXY_CACHE_TTL` | `30` | Cache TTL in seconds. Set to `0` for no caching. |
| `ENVPROXY_ENABLED` | `1` | Set to `0` to disable envproxy entirely. |
| `ENVPROXY_NO_PYTHON` | `0` | Set to `1` to skip the `os.environ` patching. |

## How the Proxy Works

- `os.getenv("KEY")` and `os.environ["KEY"]` both go through the proxy
- Keys found in the real environment are returned instantly (no agent call)
- Keys **not** in the real environment are looked up from the agent via Unix socket
- Resolved values are cached with a TTL; after expiry the agent is re-queried
- Secrets never appear in `/proc/PID/environ`
- `"KEY" in os.environ` also checks the agent for unknown keys
- Iteration (`for k in os.environ`) only returns real environment keys (agent keys are not enumerable)

## Files

| File | Description |
|------|-------------|
| `demo.py` | Live monitoring loop — shows env vars refreshing with timestamps |
| `poll.py` | Simple polling loop — prints `DATABASE_URL` and `API_KEY` every 3 seconds |
