# Java Integration

## How It Works

Java copies the process environment into an internal unmodifiable `Map` (`ProcessEnvironment.theUnmodifiableEnvironment`) at JVM startup. After that, `System.getenv()` reads from this cached map and never calls libc's `getenv()`.

To intercept `System.getenv()`, envproxy ships a **Java agent** (`envproxy-agent.jar`) that uses reflection + `sun.misc.Unsafe` to replace the internal map with a custom `EnvProxyMap` proxy.

The flow:

```
LD_PRELOAD loads libenvproxy.so
  |
  +-- .so constructor sets JAVA_TOOL_OPTIONS with:
  |     --add-opens=java.base/java.lang=ALL-UNNAMED
  |     -javaagent:/path/to/envproxy-agent.jar
  |
  v
JVM starts, picks up JAVA_TOOL_OPTIONS automatically
  |
  +-- Loads envproxy-agent.jar via -javaagent
  +-- EnvProxyAgent.premain() runs before main()
  |     +-- Uses Unsafe to replace ProcessEnvironment.theUnmodifiableEnvironment
  |     +-- Installs EnvProxyMap proxy
  |
  v
System.getenv("DATABASE_URL")
  |
  +-- EnvProxyMap.get() checks original map -> miss
  +-- queries envproxy-agent via Unix domain socket -> returns value
  +-- caches result with TTL (default: 30s)
```

## Quick Start

```bash
# Terminal 1: start the agent
mise run agent

# Terminal 2: run the demo
mise run demo:java
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `ENVPROXY_CACHE_TTL` | `30` | Cache TTL in seconds (read by the Java agent at startup). |
| `ENVPROXY_NO_JAVA` | `0` | Set to `1` to skip the `JAVA_TOOL_OPTIONS` injection. |

## How the Proxy Works

- `System.getenv("KEY")` goes through `EnvProxyMap.get()`
- Keys in the original JVM environment are returned instantly
- Missing keys are looked up from the agent via Unix domain socket (`java.nio.channels.SocketChannel` with `StandardProtocolFamily.UNIX`)
- Resolved values are cached with a TTL; after expiry the agent is re-queried
- The map is unmodifiable (throws `UnsupportedOperationException` on put/remove), matching the original behavior
- Requires Java 16+ (Unix domain socket support in `java.net`)

## Files

| File | Description |
|------|-------------|
| `Demo.java` | Live monitoring loop — shows env vars refreshing with timestamps |
