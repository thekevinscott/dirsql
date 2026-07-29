"""Resolve the bundled Rust binary inside the installed wheel."""

from __future__ import annotations

from importlib.resources import files

from dirsql.cli.is_windows import is_windows


def binary_path() -> str:
    name = "dirsql.exe" if is_windows() else "dirsql"
    # Chained rather than `joinpath("_binary", name)`: multi-segment joinpath
    # is 3.11+, and `files()` returns a bare `Traversable` for non-filesystem
    # loaders.
    path = files("dirsql").joinpath("_binary").joinpath(name)
    if not path.is_file():
        raise FileNotFoundError(
            f"bundled `{name}` not found at {path}. The dirsql PyPI wheel "
            "no longer ships the CLI binary (release-tooling regression "
            "while putitoutthere wires up bundle_cli). Install the CLI via "
            "`cargo install dirsql --features cli` or `npx dirsql`."
        )
    return str(path)
