"""Launcher-side resolution of a TOML config's ``[[dirsql.extension]]`` entries.

The compiled ``dirsql`` binary loads a config's extensions literally -- it
has no ``importlib``, so it cannot resolve a bare **package name**. When a
TOML config names an extension by package name, the shared SDK resolver
(:mod:`dirsql.resolve_config_extensions`) resolves every one of its
extensions and this launcher passes the resolved literal paths to the binary
via repeatable ``--extension`` flags; the binary then loads those and ignores
the config's own extension entries.

Native-language configs (``.py`` / ``.js`` / ``.mjs`` / ``.cjs``) are untouched:
the binary dispatches those to ``dirsql interpret``, whose handshake already
carries resolved paths.
"""

from __future__ import annotations

from ..resolve_config_extensions import resolve_config_extension_specs

# Config extensions the binary dispatches to `dirsql interpret`; never
# pre-resolved here (that path resolves via the handshake).
_NATIVE_SUFFIXES = (".py", ".js", ".mjs", ".cjs")


def _config_path_from_argv(argv: list[str]) -> str:
    """The ``--config`` value (``--config X`` or ``--config=X``), or the default."""
    for i, a in enumerate(argv):
        if a == "--config":
            # A bare trailing `--config` (no following value) yields "".
            return next(iter(argv[i + 1 :]), "")
        if a.startswith("--config="):
            return a[len("--config=") :]
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
    specs = resolve_config_extension_specs(config_path)
    if specs is None:
        return argv
    flags: list[str] = []
    for spec in specs:
        entrypoint = spec["entrypoint"]
        flags.append("--extension")
        flags.append(
            f"{spec['path']}::{entrypoint}" if entrypoint is not None else spec["path"]
        )
    return [*argv, *flags]
