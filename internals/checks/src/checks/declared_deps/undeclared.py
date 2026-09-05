"""Assert every third-party import is declared in the package's manifest (#782).

A hand-mutated local venv is strictly more capable than any real install, so an
undeclared runtime dependency is invisible locally and breaks every CI job. #777
grew `from bin_shim import main` after a `uv pip install bin-shim`, which
populates the venv and touches nothing else: 108 unit tests, 100% coverage, 27
e2e tests, `ty` clean -- then seven red jobs on `error[unresolved-import]`.

The check is static, so it costs milliseconds and needs no build: walk each
source file's imports, drop the stdlib and the package's own modules, and require
the rest to resolve to a **declared** distribution. `[dependency-groups].dev` is
allowed only in `*_test.py` files -- a dev-only dependency reached from shipped
source is the same bug wearing a different hat.
"""

from __future__ import annotations

import os.path
import sys
from collections.abc import Callable

from .declared import declared
from .gate import providers
from .top_level_imports import top_level_imports


def undeclared(
    source: str,
    manifest: dict,
    distributions: dict[str, list[str]],
    read: Callable[[str], str],
    files: list[str],
    ours: set[str],
) -> list[str]:
    """One `<file>: <module>` line per import no declared distribution provides."""
    runtime, dev = declared(manifest)
    problems = []
    for path in files:
        allowed = runtime | dev if os.path.basename(path).endswith("_test.py") else runtime
        for module in sorted(top_level_imports(read(path))):
            if module in sys.stdlib_module_names or module in ours:
                continue
            if not providers(module, distributions) & allowed:
                problems.append(f"{path}: {module}")
    return problems
