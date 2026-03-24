# Contributing to EnvProxy

Thank you for your interest in contributing to EnvProxy.

## Development Setup

### Prerequisites

Install [mise](https://mise.jdx.dev/) for toolchain management:

```bash
curl https://mise.jdx.dev/install.sh | sh
```

Then install all tools:

```bash
mise install
```

This installs:
- Rust (latest)
- Python 3.12
- Node.js 22
- Java 21

### Building

```bash
# Build all Rust crates
mise run build

# Build release binaries
mise run build:release

# Build the Java agent JAR
mise run build:java
```

### Testing

```bash
# Run Rust unit tests
mise run test

# Run clippy
mise run lint

# Full CI pipeline (format check + lint + test)
mise run ci
```

### Running Demos

```bash
# Terminal 1: start the agent
mise run agent

# Terminal 2: run a demo
mise run demo:python
mise run demo:java
mise run demo:node
mise run demo:c
```

## Project Layout

| Directory | Language | Description |
|-----------|----------|-------------|
| `crates/envproxy-proto` | Rust | Wire protocol (shared between .so and agent) |
| `crates/libenvproxy` | Rust | `LD_PRELOAD` shared library (cdylib) |
| `crates/envproxy-agent` | Rust | Local daemon with pluggable backends |
| `crates/envproxy-cli` | Rust | CLI tool (`envproxy run`, `get`, `status`) |
| `support/python` | Python | `sitecustomize.py` hook for `os.environ` patching |
| `support/java` | Java | javaagent for `System.getenv()` patching |
| `k8s/injector` | Go | Mutating admission webhook |
| `k8s/chart/envproxy` | Helm | Kubernetes Helm chart |
| `examples/` | Multi | Per-language demo scripts |

## Pull Request Guidelines

1. **One concern per PR** — keep PRs focused on a single change
2. **Run the CI checks locally** before submitting: `mise run ci`
3. **Add tests** for new functionality where applicable
4. **Update documentation** if you change behavior or add features
5. **Follow existing code style** — Rust code must pass `cargo clippy -D warnings`

### Commit Messages

Use clear, descriptive commit messages:

```
Add file watcher backend for hot-reloading secrets

The file backend now watches for mtime changes on each resolve()
call and reloads the secrets file when it changes. This enables
live secret rotation without restarting the agent.
```

## Code of Conduct

Be respectful and constructive. We follow the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct).

## License

By contributing, you agree that your contributions will be licensed under the same dual MIT/Apache-2.0 license as the project.
