#!/usr/bin/env python3
"""Polling loop that prints env vars every 3 seconds."""

import os
import time

while True:
    db = os.getenv("DATABASE_URL", "<NOT SET>")
    api = os.getenv("API_KEY", "<NOT SET>")
    ts = time.strftime("%H:%M:%S")
    print(f"[{ts}] DATABASE_URL={db}")
    print(f"[{ts}] API_KEY={api}")
    print("---")
    time.sleep(3)
