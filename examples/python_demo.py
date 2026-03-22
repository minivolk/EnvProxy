#!/usr/bin/env python3
"""
Demo: reading environment variables through envproxy.

Usage:
    # Start the agent first:
    envproxy-agent --config examples/config.toml

    # Then run this script with envproxy:
    envproxy run -- python3 examples/python_demo.py

    # Or manually:
    LD_PRELOAD=target/release/libenvproxy.so python3 examples/python_demo.py
"""

import os

keys = ["DATABASE_URL", "API_KEY", "REDIS_URL", "JWT_SECRET", "NONEXISTENT_KEY"]

for key in keys:
    value = os.getenv(key, "<not set>")
    # Mask secrets in output for safety
    if value != "<not set>" and len(value) > 8:
        display = value[:4] + "..." + value[-4:]
    else:
        display = value
    print(f"  {key} = {display}")
