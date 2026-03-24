"""
envproxy site customization — auto-patches os.environ on Python startup.

This file is auto-loaded by Python's site.py when found in PYTHONPATH.
It imports _envproxy_hook (which patches os.environ), then chains to any
pre-existing sitecustomize module that may exist elsewhere in sys.path.

The chaining ensures we don't break existing sitecustomize.py files from
other tools (e.g., coverage, virtualenvs).
"""

import os
import importlib
import sys

# Step 1: Import our hook to patch os.environ.
try:
    import _envproxy_hook  # noqa: F401 — imported for side effect
except Exception:
    # If the hook fails for any reason, don't break Python startup.
    pass

# Step 2: Chain to any pre-existing sitecustomize module.
# We must temporarily remove our directory from sys.path so the import
# machinery doesn't find our own sitecustomize.py again (infinite recursion).
_our_file = __file__
_our_dir = os.path.dirname(os.path.abspath(_our_file))
_our_module = sys.modules.pop("sitecustomize", None)

_path_modified = False
if _our_dir in sys.path:
    sys.path.remove(_our_dir)
    _path_modified = True

try:
    importlib.import_module("sitecustomize")
except ImportError:
    # No other sitecustomize exists — that's fine.
    pass
finally:
    if _path_modified:
        sys.path.insert(0, _our_dir)
    if _our_module is not None:
        sys.modules["sitecustomize"] = _our_module
