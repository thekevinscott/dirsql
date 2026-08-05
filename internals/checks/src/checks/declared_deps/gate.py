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

Import name and distribution name differ (`yaml` ships in `pyyaml`), so the
mapping comes from `importlib.metadata.packages_distributions()` rather than a
hand-kept table that would drift. The installed environment supplies only the
*name* mapping; whether a dependency is declared is read from the manifest.
"""

from __future__ import annotations

import ast
import os
import os.path
import sys
import tomllib
from collections.abc import Callable, Iterable
from importlib.metadata import packages_distributions


def normalize(name: str) -> str:
    """PEP 503 name normalization, so `bin-shim` and `bin_shim` are one name."""
    return name.lower().replace("_", "-")


def requirement_name(spec: str) -> str:
    """The distribution name from a requirement string, dropping any version/extras."""
    for separator in ("[", "<", ">", "=", "!", "~", ";", " "):
        spec = spec.split(separator)[0]
    return normalize(spec)


def top_level_imports(text: str) -> set[str]:
    """Top-level module names imported by a source file; relative imports are ours."""
    names = set()
    for node in ast.walk(ast.parse(text)):
        if isinstance(node, ast.Import):
            names.update(alias.name.split(".")[0] for alias in node.names)
        elif isinstance(node, ast.ImportFrom) and not node.level and node.module:
            names.add(node.module.split(".")[0])
    return names


def declared(manifest: dict) -> tuple[set[str], set[str]]:
    """(runtime, dev) distribution names the manifest declares."""
    runtime = {requirement_name(s) for s in manifest.get("project", {}).get("dependencies", [])}
    groups = manifest.get("dependency-groups", {})
    dev = {requirement_name(s) for s in groups.get("dev", [])}
    return runtime, dev


def providers(module: str, distributions: dict[str, list[str]]) -> set[str]:
    """Declared-name candidates for an import: its distributions, else itself."""
    return {normalize(name) for name in distributions.get(module, [module])}


def source_files(source: str, walk: Callable[[str], Iterable] = os.walk) -> list[str]:
    found = []
    for directory, _subdirs, names in walk(source):
        found += [os.path.join(directory, n) for n in sorted(names) if n.endswith(".py")]
    return sorted(found)


def package_root(source: str, exists: Callable[[str], bool] = os.path.exists) -> str:
    """Nearest ancestor of `source` (inclusive) holding a pyproject.toml."""
    parts = source.split("/")
    while parts:
        candidate = "/".join(parts)
        if exists(f"{candidate}/pyproject.toml"):
            return candidate
        parts.pop()
    return "."


def first_party(source: str, listdir: Callable[[str], Iterable[str]] = os.listdir) -> set[str]:
    """Top-level names the scanned tree itself defines -- never a dependency."""
    names = {os.path.basename(source.rstrip("/"))}
    for entry in listdir(source):
        names.add(entry[:-3] if entry.endswith(".py") else entry)
    return names


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


def read_text(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()


def read_manifest(path: str) -> dict:
    with open(path, "rb") as handle:
        return tomllib.load(handle)


def warn(line: str) -> None:
    print(line, file=sys.stderr)


def run(
    source: str,
    *,
    manifest: Callable[[str], dict] = read_manifest,
    distributions: Callable[[], dict] = packages_distributions,
    read: Callable[[str], str] = read_text,
    files: Callable[[str], list[str]] = source_files,
    ours: Callable[[str], set[str]] = first_party,
    echo: Callable[[str], None] = warn,
) -> int:
    root = package_root(source)
    problems = undeclared(
        source,
        manifest(f"{root}/pyproject.toml"),
        distributions(),
        read,
        files(source),
        ours(source),
    )
    for problem in problems:
        echo(f"undeclared dependency -- {problem}")
    if problems:
        echo(
            f"declared-deps: {len(problems)} import(s) not declared in {root}/pyproject.toml. "
            "Add each to [project].dependencies (or [dependency-groups].dev for a "
            "test-only import) and run `uv sync` -- never `uv pip install`, which "
            "populates the venv without declaring anything."
        )
        return 1
    return 0
