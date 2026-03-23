#!/usr/bin/env python3
"""
Demo: live environment variable monitoring through envproxy.

Shows secrets being resolved dynamically from the agent.
Edit examples/secrets.json while this is running to see live rotation.

Usage:
    # Quick start with mise:
    mise run demo

    # Or manually:
    envproxy-agent --config examples/config.toml &
    ENVPROXY_PYTHON_PATH=$(pwd)/python \\
    ENVPROXY_CACHE_TTL=5 \\
    LD_PRELOAD=target/release/libenvproxy.so \\
    python3 examples/python_demo.py
"""

import os
import time

KEYS = ["DATABASE_URL", "API_KEY", "REDIS_URL", "JWT_SECRET"]


def mask(value):
    """Mask the middle of a secret for safe display."""
    if value is None or len(value) <= 12:
        return value
    return value[:6] + "..." + value[-6:]


print(f"envproxy Python demo (os.environ type: {type(os.environ).__name__})")
print(f"Cache TTL: {os.getenv('ENVPROXY_CACHE_TTL', '30')}s")
print("Edit examples/secrets.json to see live rotation. Ctrl+C to stop.")
print()

try:
    while True:
        ts = time.strftime("%H:%M:%S")
        for key in KEYS:
            value = os.getenv(key, "<not set>")
            print(f"  [{ts}] {key} = {mask(value)}")
        print()
        time.sleep(1)
except KeyboardInterrupt:
    print("\nStopped.")
