"""
envproxy Python hook — patches os.environ to query envproxy-agent for missing keys.

This module is auto-loaded via sitecustomize.py during Python startup. It replaces
os.environ with a proxy that transparently resolves environment variables from
the envproxy-agent daemon when they are not found in the real process environment.

The proxy:
- Returns real env vars instantly (no agent call) for keys already in environ
- Queries the agent via Unix socket only for keys NOT in the real environ
- Caches resolved values with a configurable TTL (default: 30s)
- Automatically re-queries the agent after the TTL expires
- Is fully compatible with os.getenv(), os.environ.get(), os.environ["KEY"], etc.

Wire protocol (matches envproxy-proto):
  Request:  [1: version] [2: key_len BE] [N: key]
  Response: [1: status]  [2: val_len BE] [N: value]
  Status: 0x00=Found, 0x01=NotFound, 0x02=Error, 0x03=Passthrough
"""

import os
import socket
import struct
import time
from collections.abc import MutableMapping

_PROTOCOL_VERSION = 1
_STATUS_FOUND = 0x00

_DEFAULT_SOCKET_PATH = "/tmp/envproxy/agent.sock"
_CONNECT_TIMEOUT = 0.5
_READ_TIMEOUT = 0.5
_DEFAULT_CACHE_TTL = 30.0  # seconds


def _query_agent(key, sock_path):
    """Query the envproxy-agent for a key via Unix socket.

    Returns the value as a string, or None if not found / agent unavailable.
    """
    key_bytes = key.encode("utf-8", errors="surrogateescape")
    key_len = len(key_bytes)

    if key_len > 0xFFFF:
        return None

    # Build request: [version:1][key_len:2 BE][key:N]
    request = struct.pack(">BH", _PROTOCOL_VERSION, key_len) + key_bytes

    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            sock.settimeout(_CONNECT_TIMEOUT)
            sock.connect(sock_path)
            sock.settimeout(_READ_TIMEOUT)
            sock.sendall(request)

            # Read response header: [status:1][val_len:2 BE]
            header = _recv_exact(sock, 3)
            if header is None:
                return None

            status = header[0]
            val_len = struct.unpack(">H", header[1:3])[0]

            # Read value
            if val_len > 0:
                value_bytes = _recv_exact(sock, val_len)
                if value_bytes is None:
                    return None
            else:
                value_bytes = b""

            if status == _STATUS_FOUND:
                return value_bytes.decode("utf-8", errors="surrogateescape")

            return None
        finally:
            sock.close()

    except (OSError, ConnectionError, TimeoutError):
        return None


def _recv_exact(sock, n):
    """Receive exactly n bytes from a socket, or return None on failure."""
    buf = bytearray()
    while len(buf) < n:
        try:
            chunk = sock.recv(n - len(buf))
        except (OSError, TimeoutError):
            return None
        if not chunk:
            return None
        buf.extend(chunk)
    return bytes(buf)


class _CacheEntry:
    """A cached value with a timestamp for TTL-based expiry."""

    __slots__ = ("value", "timestamp")

    def __init__(self, value, timestamp):
        self.value = value
        self.timestamp = timestamp


class _EnvProxyEnviron(MutableMapping):
    """A proxy around os._Environ that queries envproxy-agent for missing keys.

    Delegates all operations to the original os.environ instance, but intercepts
    ``__getitem__`` to query the agent when a key is not found in the real
    environment.

    Resolved values are cached with a TTL. After the TTL expires, the next
    lookup for that key will re-query the agent, picking up rotated secrets.
    """

    _envproxy = True  # Marker attribute to detect already-patched instances.

    def __init__(self, original, sock_path, cache_ttl=_DEFAULT_CACHE_TTL):
        self._original = original
        self._cache = {}
        self._sock_path = sock_path
        self._cache_ttl = cache_ttl

    def _is_cache_valid(self, entry):
        """Check if a cache entry is still within its TTL."""
        return (time.monotonic() - entry.timestamp) < self._cache_ttl

    def __getitem__(self, key):
        # Fast path: try the real environ dict.
        try:
            real_value = self._original[key]
            # If the value starts with "vault:", it's a Vault reference
            # that needs to be resolved by the agent.
            if not real_value.startswith("vault:"):
                return real_value
            # Fall through to agent resolution below.
        except KeyError:
            real_value = None

        # Check our local cache of agent-resolved values.
        entry = self._cache.get(key)
        if entry is not None and self._is_cache_valid(entry):
            if entry.value is None:
                raise KeyError(key)
            return entry.value

        # Query the agent (cache miss or expired).
        value = _query_agent(key, self._sock_path)
        self._cache[key] = _CacheEntry(value, time.monotonic())
        if value is not None:
            return value

        raise KeyError(key)

    def __setitem__(self, key, value):
        # Writes go to the real environ (which calls putenv internally).
        self._original[key] = value
        # Invalidate cache for this key.
        self._cache.pop(key, None)

    def __delitem__(self, key):
        self._original.__delitem__(key)
        self._cache.pop(key, None)

    def __iter__(self):
        return iter(self._original)

    def __len__(self):
        return len(self._original)

    def __contains__(self, key):
        if key in self._original:
            return True
        # Check agent for this key.
        try:
            self[key]
            return True
        except KeyError:
            return False

    def __repr__(self):
        return f"_EnvProxyEnviron({self._original!r})"

    def copy(self):
        """Return a plain dict snapshot of the real environment."""
        return self._original.copy()


def _install():
    """Replace os.environ with the envproxy proxy."""
    # Guard: don't patch if already patched.
    if getattr(os.environ, "_envproxy", False):
        return

    # Check if envproxy is disabled via ENVPROXY_ENABLED=0.
    enabled = os.environ.get("ENVPROXY_ENABLED", "1")
    if enabled == "0":
        return

    # Determine socket path from the real environ (before we wrap it).
    sock_path = os.environ.get("ENVPROXY_SOCKET", _DEFAULT_SOCKET_PATH)

    # Read cache TTL from environment (default: 30 seconds).
    try:
        cache_ttl = float(os.environ.get("ENVPROXY_CACHE_TTL", str(_DEFAULT_CACHE_TTL)))
    except (ValueError, TypeError):
        cache_ttl = _DEFAULT_CACHE_TTL

    # Check if the agent socket exists (don't patch if agent isn't running).
    if not os.path.exists(sock_path):
        return

    original = os.environ
    proxy = _EnvProxyEnviron(original, sock_path, cache_ttl)
    os.environ = proxy


# Auto-install when this module is imported (triggered by sitecustomize.py).
_install()
