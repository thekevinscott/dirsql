"""Resolve the bundled Rust binary inside the installed wheel."""

from __future__ import annotations

from importlib.resources import files

from dirsql._cli.is_windows import is_windows


def binary_path() -> str:
    name = "dirsql.exe" if is_windows() else "dirsql"
    path = files("dirsql").joinpath("_binary", name)
    if not path.is_file():
        raise FileNotFoundError(
            f"bundled `{name}` not found at {path}; "
            "this wheel was built without the CLI binary. "
            "Rebuild with `maturin build --release --bin dirsql --features cli`."
        )
    return str(path)
