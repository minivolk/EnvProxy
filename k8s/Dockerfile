# envproxy container image
# Contains: envproxy-agent, envproxy CLI, libenvproxy.so, python/ hook, java/ agent
#
# Used as:
#   - Sidecar: envproxy-agent runs alongside app containers in each pod
#   - Init container: runs "envproxy init --target /envproxy" to copy runtime files
#
# Build modes:
#   Local:  docker build -f k8s/Dockerfile .
#           (builds Rust + Java inside Docker, slow but self-contained)
#   CI:     Pre-built binaries are placed in binaries/{amd64,arm64}/ by the
#           release workflow. The Rust build stage is skipped via cache hit.
#
# Runtime is distroless/cc-debian12 (glibc included for libenvproxy.so).
# No shell — the init container uses "envproxy init" subcommand instead of sh -c.

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
RUN cargo build --release --features full && \
    strip target/release/envproxy-agent && \
    strip target/release/envproxy && \
    strip target/release/libenvproxy.so

# ── Runtime (distroless, non-root — no shell, minimal attack surface) ─
FROM gcr.io/distroless/cc-debian12:nonroot

# Rust binaries — from CI pre-built artifacts or from rust-builder stage.
ARG TARGETARCH
COPY binaries/${TARGETARCH}/envproxy-agent  /usr/bin/envproxy-agent
COPY binaries/${TARGETARCH}/envproxy        /usr/bin/envproxy
COPY binaries/${TARGETARCH}/libenvproxy.so  /usr/lib/envproxy/lib/libenvproxy.so

# Python hook
COPY support/python/ /usr/lib/envproxy/python/

# Java agent JAR
COPY --from=java-builder /src/envproxy-agent.jar /usr/lib/envproxy/java/envproxy-agent.jar

USER nonroot:nonroot

ENTRYPOINT ["envproxy-agent"]
CMD ["--config", "/etc/envproxy/config.toml"]
