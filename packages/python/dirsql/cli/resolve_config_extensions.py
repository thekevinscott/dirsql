"""Launcher-side resolution of a TOML config's ``[[dirsql.extension]]`` entries.

Mirrors the TypeScript launcher. The compiled ``dirsql`` binary reads a
``.dirsql.toml`` itself and loads its extensions literally -- it has no
``importlib``, so it cannot resolve a bare **package name** (#227). This
launcher can. When a TOML config names an extension by package name, we resolve
every one of its extensions here and pass the resolved literal paths to the
binary via repeatable ``--extension`` flags; the binary then loads those and
ignores the config's own extension entries (the Rust ``--extension`` flag /
``suppress_config_extensions``).

Native-language configs (``.py`` / ``.js`` / ``.mjs`` / ``.cjs``) are untouched:
the binary dispatches those to ``dirsql interpret``, whose handshake already
carries resolved paths.
"""

from __future__ import annotations

import os
import tomllib

from ..resolve_extension import is_bare_name, resolve_extension_path

# Config extensions the binary dispatches to `dirsql interpret`; never
# pre-resolved here (that path resolves via the handshake).
_NATIVE_SUFFIXES = (".py", ".js", ".mjs", ".cjs")


def _config_path_from_argv(argv: list[str]) -> str:
    """The ``--config`` value (``--config X`` or ``--config=X``), or the default."""
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--config":
            return argv[i + 1] if i + 1 < len(argv) else ""
        if a.startswith("--config="):
            return a[len("--config=") :]
        i += 1
    return "./.dirsql.toml"


def with_resolved_extensions(argv: list[str]) -> list[str]:
    """Return ``argv`` plus ``--extension`` flags when the TOML config names an
    extension by package name; otherwise return ``argv`` unchanged. Raises if a
    package name cannot be resolved (the launcher surfaces a clean error)."""
    if argv and argv[0] == "init":
        return argv
    config_path = _config_path_from_argv(argv)
    if config_path.endswith(_NATIVE_SUFFIXES):
        return argv
    if not os.path.isfile(config_path):
        return argv
    try:
        with open(config_path, "rb") as f:
            doc = tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError):
        # Leave a malformed / unreadable config for the binary to report.
        return argv

    cfg = doc.get("dirsql") or {}
    entries = cfg.get("extension") or []
    if not isinstance(entries, list) or not entries:
        return argv
    # Only intervene when at least one path is a bare package name; a config
    # with only literal paths keeps the binary's existing behavior untouched.
    if not any(
        isinstance(e, dict)
        and isinstance(e.get("path"), str)
        and is_bare_name(e["path"])
        for e in entries
    ):
        return argv

    base = os.path.dirname(os.path.abspath(config_path))
    flags: list[str] = []
    for e in entries:
        path = resolve_extension_path(e["path"], base=base, resolve_relative=True)
        entrypoint = e.get("entrypoint")
        flags.append("--extension")
        flags.append(f"{path}::{entrypoint}" if isinstance(entrypoint, str) else path)
    return [*argv, *flags]
