"""Resolve a plugin module's shipped ``dirsql.toml`` fragment path."""

from __future__ import annotations

from importlib import resources

FRAGMENT_NAME = "dirsql.toml"


def fragment_path(module_name: str) -> str:
    """Absolute path to a plugin module's shipped ``dirsql.toml``. Raises a
    clear error naming the plugin when the module or the fragment is missing --
    never a silent skip."""
    try:
        fragment = resources.files(module_name).joinpath(FRAGMENT_NAME)
    except ModuleNotFoundError as exc:
        raise ValueError(
            f"dirsql plugin module {module_name!r} is not importable: {exc}"
        ) from exc
    if not fragment.is_file():
        raise ValueError(
            f"dirsql plugin {module_name!r} ships no {FRAGMENT_NAME} fragment "
            f"(expected at {fragment})"
        )
    return str(fragment)
