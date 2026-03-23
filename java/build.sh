#!/usr/bin/env bash
# Build the envproxy Java agent JAR.
# Output: java/envproxy-agent.jar
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BUILD_DIR="$SCRIPT_DIR/build"

rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"

# Compile (sun.misc.Unsafe comes from jdk.unsupported module automatically).
javac \
    -d "$BUILD_DIR" \
    "$SCRIPT_DIR/src/envproxy/EnvProxyAgent.java" \
    "$SCRIPT_DIR/src/envproxy/EnvProxyMap.java"

# Package JAR.
jar cfm "$SCRIPT_DIR/envproxy-agent.jar" \
    "$SCRIPT_DIR/META-INF/MANIFEST.MF" \
    -C "$BUILD_DIR" .

rm -rf "$BUILD_DIR"
echo "Built: $SCRIPT_DIR/envproxy-agent.jar"
