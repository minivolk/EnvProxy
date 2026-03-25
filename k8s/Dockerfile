# envproxy container image
# Contains: envproxy-agent, envproxy CLI, libenvproxy.so, python/ hook, java/ agent
#
# Used as:
#   - DaemonSet: ENTRYPOINT ["envproxy-agent"]
#   - Init container: copies /usr/lib/envproxy/* into a shared emptyDir volume
#
# Build modes:
#   Local:  docker build -f k8s/Dockerfile .
#           (builds Rust + Java inside Docker, slow but self-contained)
#   CI:     Pre-built binaries are placed in binaries/{amd64,arm64}/ by the
#           release workflow. The Rust build stage is skipped via cache hit.
#
# NOTE: Runtime is debian (not alpine) because libenvproxy.so and the CLI
# must be glibc-linked — they get copied into glibc-based app pods.

# ── Java build stage (architecture-independent) ──────────────────────
FROM eclipse-temurin:21-jdk AS java-builder
WORKDIR /src
COPY support/java/src/ src/
COPY support/java/META-INF/ META-INF/
RUN mkdir -p build && \
    javac -d build src/envproxy/EnvProxyAgent.java src/envproxy/EnvProxyMap.java && \
    jar cfm envproxy-agent.jar META-INF/MANIFEST.MF -C build .

# ── Rust build stage (for local builds; CI uses pre-built binaries) ──
FROM rust:1.88-trixie AS rust-builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --release --features kubernetes && \
    strip target/release/envproxy-agent && \
    strip target/release/envproxy && \
    strip target/release/libenvproxy.so

# ── Runtime ──────────────────────────────────────────────────────────
FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Rust binaries — from CI pre-built artifacts or from rust-builder stage.
# In CI, the release workflow places binaries in binaries/{amd64,arm64}/
# and buildx sets TARGETARCH automatically.
ARG TARGETARCH
COPY binaries/${TARGETARCH}/envproxy-agent  /usr/bin/envproxy-agent
COPY binaries/${TARGETARCH}/envproxy        /usr/bin/envproxy
COPY binaries/${TARGETARCH}/libenvproxy.so  /usr/lib/envproxy/lib/libenvproxy.so

# Python hook
COPY support/python/ /usr/lib/envproxy/python/

# Java agent JAR
COPY --from=java-builder /src/envproxy-agent.jar /usr/lib/envproxy/java/envproxy-agent.jar

RUN mkdir -p /var/run/envproxy && \
    chmod +x /usr/bin/envproxy-agent /usr/bin/envproxy

ENTRYPOINT ["envproxy-agent"]
CMD ["--config", "/etc/envproxy/config.toml"]
