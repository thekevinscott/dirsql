"""Stub the compiled `_dirsql` extension for unit tests that don't need it.

`_cli_test.py` exercises the pure-Python launcher; it must be runnable
without `maturin develop` having built the PyO3 extension. Stubbing here
(rather than in the test file) ensures the stub is installed before pytest
imports `dirsql.__init__`, which transitively imports real types from
`dirsql._dirsql`.

Tests that exercise the real SDK bindings (`test_async.py`,
`tests/integration/`) must be run after `maturin develop` — they are not
served by this stub and assert on real extension behavior.
"""

from __future__ import annotations

import sys
import types

if "dirsql._dirsql" not in sys.modules:
    _stub = types.ModuleType("dirsql._dirsql")
    _stub.__version__ = "9.9.9-test"
    _stub.Table = type("Table", (), {})
    _stub.RowEvent = type("RowEvent", (), {})
    _stub.DirSQL = type("DirSQL", (), {})
    sys.modules["dirsql._dirsql"] = _stub
