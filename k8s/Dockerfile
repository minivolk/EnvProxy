# envproxy container image
# Contains: envproxy-agent, envproxy CLI, libenvproxy.so, python/ hook, java/ agent
#
# Used as:
#   - DaemonSet: ENTRYPOINT ["envproxy-agent"]
#   - Init container: copies /usr/lib/envproxy/* into a shared emptyDir volume
#
# NOTE: Runtime is debian-slim (not alpine) because libenvproxy.so and the
# CLI binary must be glibc-linked — they get copied into app pods (python,
# node, java) which are all glibc-based.

FROM rust:1.88-trixie AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Build with kubernetes feature enabled
RUN cargo build --release --features kubernetes \
  && strip target/release/envproxy-agent \
  && strip target/release/envproxy \
  && strip target/release/libenvproxy.so

# -------------------------------------------------------------------
FROM debian:trixie-slim

ENV DEBIAN_FRONTEND noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
  ca-certificates \
  && rm -rf /var/lib/apt/lists/*

# Agent and CLI binaries
COPY --from=builder /src/target/release/envproxy-agent /usr/bin/envproxy-agent
COPY --from=builder /src/target/release/envproxy /usr/bin/envproxy

# Runtime files for LD_PRELOAD injection
COPY --from=builder /src/target/release/libenvproxy.so /usr/lib/envproxy/lib/libenvproxy.so

# Python hook (sitecustomize.py + _envproxy_hook.py)
COPY support/python/ /usr/lib/envproxy/python/

# Java agent JAR (build with: cd support/java && bash build.sh)
COPY support/java/envproxy-agent.jar /usr/lib/envproxy/java/envproxy-agent.jar

# Default socket directory
RUN mkdir -p /var/run/envproxy

ENTRYPOINT ["envproxy-agent"]
CMD ["--config", "/etc/envproxy/config.toml"]
