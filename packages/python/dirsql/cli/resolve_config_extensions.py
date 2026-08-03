"""Launcher-side resolution of the TOML configs' ``[[dirsql.extension]]`` entries.

The compiled ``dirsql`` binary loads a config's extensions literally -- it
has no ``importlib``, so it cannot resolve a bare **package name**. When any
TOML config in argv names an extension by package name, the shared SDK
resolver (:mod:`dirsql.resolve_config_extensions`) resolves every config's
extensions and this launcher passes the resolved literal paths to the binary
via repeatable ``--extension`` flags; the binary then loads those and ignores
the configs' own extension entries.

Every config flag occurrence counts -- ``-c``/``--config`` are repeatable,
and plugin discovery injects fragments as additional ``-c`` flags -- so the
scan collects them all, in argv order.

Native-language configs (``.py`` / ``.js`` / ``.mjs`` / ``.cjs``) are untouched:
the binary dispatches those to ``dirsql interpret``, whose handshake already
carries resolved paths.
"""

from __future__ import annotations

from ..resolve_config_extensions import resolve_configs_extension_specs

# Config extensions the binary dispatches to `dirsql interpret`; never
# pre-resolved here (that path resolves via the handshake).
_NATIVE_SUFFIXES = (".py", ".js", ".mjs", ".cjs")


def _config_paths_from_argv(argv: list[str]) -> list[str]:
    """Every config value in argv, in order (``--config X``, ``--config=X``,
    ``-c X``, ``-c=X``, ``-cX``), or the default when none are given."""
    paths: list[str] = []
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--config" or a == "-c":
            # A bare trailing flag (no following value) yields "".
            paths.append(next(iter(argv[i + 1 :]), ""))
            i += 2
            continue
        if a.startswith("--config="):
            paths.append(a[len("--config=") :])
        elif a.startswith("-c"):
            paths.append(a[len("-c") :].removeprefix("="))
        i += 1
    return paths or ["./.dirsql.toml"]


def with_resolved_extensions(argv: list[str]) -> list[str]:
    """Return ``argv`` plus ``--extension`` flags when a TOML config names an
    extension by package name; otherwise return ``argv`` unchanged. Raises if a
    package name cannot be resolved (the launcher surfaces a clean error)."""
    if argv and argv[0] == "init":
        return argv
    config_paths = [
        p for p in _config_paths_from_argv(argv) if not p.endswith(_NATIVE_SUFFIXES)
    ]
    if not config_paths:
        return argv
    specs = resolve_configs_extension_specs(config_paths)
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
