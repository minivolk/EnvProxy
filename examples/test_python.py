#!/usr/bin/env python3
"""
envproxy Python integration test.

Python caches environ at startup, so envproxy uses a sitecustomize.py hook
to replace os.environ with a proxy that queries the agent for missing keys.

Usage:
    ENVPROXY_PYTHON_PATH=$(pwd)/python \
    ENVPROXY_CACHE_TTL=0 \
    LD_PRELOAD=target/release/libenvproxy.so \
    python3 examples/test_python.py
"""

import os
import sys

EXPECTED = {
    "DATABASE_URL": "postgres://user:secret@localhost:5432/mydb",
    "API_KEY": "sk-envproxy-demo-1234567890",
    "REDIS_URL": "redis://localhost:6379/0",
    "JWT_SECRET": "super-secret-jwt-signing-key",
}

passed = 0
failed = 0


def check(key, expected):
    global passed, failed
    actual = os.getenv(key)
    if actual == expected:
        passed += 1
    else:
        print(f"FAIL: {key} = {actual!r} (expected {expected!r})", file=sys.stderr)
        failed += 1


def check_none(key):
    global passed, failed
    actual = os.getenv(key)
    if actual is None:
        passed += 1
    else:
        print(f"FAIL: {key} = {actual!r} (expected None)", file=sys.stderr)
        failed += 1


def check_not_empty(key):
    global passed, failed
    actual = os.getenv(key)
    if actual is not None and len(actual) > 0:
        passed += 1
    else:
        print(f"FAIL: {key} = {actual!r} (expected non-empty)", file=sys.stderr)
        failed += 1


# Verify the proxy is installed.
environ_type = type(os.environ).__name__
if environ_type != "_EnvProxyEnviron":
    print(
        f"FAIL: os.environ type is {environ_type} (expected _EnvProxyEnviron)",
        file=sys.stderr,
    )
    failed += 1
else:
    passed += 1

# Test all secret keys.
for key, expected in EXPECTED.items():
    check(key, expected)

# Missing key should return None.
check_none("NONEXISTENT_KEY_12345")

# Real env vars should still work.
check_not_empty("HOME")

# dict-style access should work.
try:
    val = os.environ["DATABASE_URL"]
    if val == EXPECTED["DATABASE_URL"]:
        passed += 1
    else:
        print(f"FAIL: os.environ['DATABASE_URL'] = {val!r}", file=sys.stderr)
        failed += 1
except KeyError:
    print("FAIL: os.environ['DATABASE_URL'] raised KeyError", file=sys.stderr)
    failed += 1

# 'in' operator should work.
if "DATABASE_URL" in os.environ:
    passed += 1
else:
    print("FAIL: 'DATABASE_URL' in os.environ returned False", file=sys.stderr)
    failed += 1

# Secrets must NOT be in /proc/self/environ.
try:
    with open("/proc/self/environ", "rb") as f:
        proc_env = f.read()
    # Check none of our secret values leak into the process environment block.
    for key in EXPECTED:
        if key.encode() in proc_env:
            # Key name might exist as part of other vars (e.g., ENVPROXY_PYTHON_PATH).
            # Check for KEY=VALUE pattern specifically.
            pattern = f"{key}={EXPECTED[key]}".encode()
            if pattern in proc_env:
                print(
                    f"FAIL: {key} found in /proc/self/environ (secret leak!)",
                    file=sys.stderr,
                )
                failed += 1
            else:
                passed += 1
        else:
            passed += 1
except OSError:
    # /proc/self/environ not available (non-Linux) — skip.
    pass

if failed > 0:
    print(f"Python: {passed} passed, {failed} failed", file=sys.stderr)
    sys.exit(1)
else:
    print(f"Python: {passed} passed")
