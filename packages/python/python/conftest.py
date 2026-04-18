"""Stub the compiled `_dirsql` extension for unit tests that don't need it.

`_cli_test.py` exercises the pure-Python launcher; it must be runnable
without `maturin develop` having built the PyO3 extension. We stub the
missing module here (rather than in the test file) so the stub is in
place before pytest imports `dirsql.__init__`, which transitively
imports real types from `dirsql._dirsql`.

The stub is installed ONLY when the real extension can't be imported.
When `maturin develop` has been run (as in CI), the real extension wins
and tests that depend on it — `test_async.py`, the integration suites —
see the real bindings.
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
