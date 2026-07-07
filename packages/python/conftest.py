"""Stub the compiled `_dirsql` extension for unit tests that don't need it.

Must live here (not in a test file) so the stub is in place before pytest
imports `dirsql.__init__`, which imports real types from `dirsql._dirsql`.
Installed only when the real extension can't be imported; when `maturin
develop` has been run, the real bindings win.
"""

from __future__ import annotations

import importlib
import sys
import types


def _try_import_real() -> bool:
    try:
        importlib.import_module("dirsql._dirsql")
    except ImportError:
        return False
    return True


if not _try_import_real():
    _stub = types.ModuleType("dirsql._dirsql")
    _stub.__version__ = "9.9.9-test"
    _stub.Table = type("Table", (), {})
    _stub.RowEvent = type("RowEvent", (), {})
    _stub.DirSQL = type("DirSQL", (), {})
    sys.modules["dirsql._dirsql"] = _stub
