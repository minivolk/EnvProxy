"""
envproxy site customization — auto-patches os.environ on Python startup.

This file is auto-loaded by Python's site.py when found in PYTHONPATH.
It imports _envproxy_hook (which patches os.environ), then chains to any
pre-existing sitecustomize module that may exist elsewhere in sys.path.

The chaining ensures we don't break existing sitecustomize.py files from
other tools (e.g., coverage, virtualenvs).
"""

import importlib
import sys

# Step 1: Import our hook to patch os.environ.
try:
    import _envproxy_hook  # noqa: F401 — imported for side effect
except Exception:
    # If the hook fails for any reason, don't break Python startup.
    pass

# Step 2: Chain to any pre-existing sitecustomize module.
# Remove ourselves from sys.modules so the import machinery can find
# the "real" sitecustomize (if any) from other paths.
_our_file = __file__
_our_module = sys.modules.pop("sitecustomize", None)

try:
    # Try to import the next sitecustomize in the path.
    importlib.import_module("sitecustomize")
except ImportError:
    # No other sitecustomize exists — that's fine.
    pass
finally:
    # Restore ourselves in sys.modules.
    if _our_module is not None:
        sys.modules["sitecustomize"] = _our_module
