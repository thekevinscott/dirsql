"""Resolve the bundled Rust binary inside the installed wheel."""

from __future__ import annotations

from importlib.resources import files
from typing import Any, Callable

from dirsql.cli.is_windows import is_windows


def _default_package_root() -> Any:
    return files("dirsql")


def binary_path(
    *,
    is_windows_fn: Callable[[], bool] = is_windows,
    package_root: Callable[[], Any] = _default_package_root,
) -> str:
    name = "dirsql.exe" if is_windows_fn() else "dirsql"
    path = package_root().joinpath("_binary", name)
    if not path.is_file():
        raise FileNotFoundError(
            f"bundled `{name}` not found at {path}. The dirsql PyPI wheel "
            "no longer ships the CLI binary (release-tooling regression "
            "while putitoutthere wires up bundle_cli). Install the CLI via "
            "`cargo install dirsql --features cli` or `npx dirsql`."
        )
    return str(path)
