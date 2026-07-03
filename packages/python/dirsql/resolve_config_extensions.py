"""SDK-side resolution of a TOML config's ``[[dirsql.extension]]`` entries.

The Rust core parses a ``.dirsql.toml`` itself and loads its extensions
literally -- it has no ``importlib``, so it cannot resolve a bare **package
name** (#227). The SDK can (#313). When a TOML config names an extension by
package name, the SDK resolves every one of its extensions here, hands the
core the resolved literal paths, and suppresses the core's own config-extension
loading (the Rust ``suppress_config_extensions`` builder toggle) so the
config's entries are not loaded a second time.

Shared by the ``DirSQL`` constructor (``config=`` path) and the CLI launcher
(``dirsql.cli.resolve_config_extensions``, which converts the resolved specs
into ``--extension`` flags for the binary).
"""

from __future__ import annotations

import os
import tomllib

from .resolve_extension import is_bare_name, resolve_extension_path


def resolve_config_extension_specs(config_path):
    """Resolve a TOML config's ``[[dirsql.extension]]`` entries to literal paths.

    Returns a list of ``{"path", "entrypoint"}`` dicts -- every entry resolved
    via :func:`resolve_extension_path` against the config file's parent
    directory -- when at least one entry's ``path`` is a bare package name.
    Returns ``None`` when the caller should not intervene: the config is
    missing, malformed, declares no extensions, or uses only literal paths --
    leaving the core's own loading (and error reporting) untouched. Raises if
    a package name cannot be resolved.
    """
    if not os.path.isfile(config_path):
        return None
    try:
        with open(config_path, "rb") as f:
            doc = tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError):
        # Leave a malformed / unreadable config for the core to report.
        return None

    cfg = doc.get("dirsql") or {}
    entries = cfg.get("extension")
    if not isinstance(entries, list):
        return None
    # Only intervene when at least one path is a bare package name; a config
    # with only literal paths (or no entries at all) keeps the core's existing
    # behavior untouched.
    if not any(
        isinstance(e, dict)
        and isinstance(e.get("path"), str)
        and is_bare_name(e["path"])
        for e in entries
    ):
        return None

    base = os.path.dirname(os.path.abspath(config_path))
    specs = []
    for e in entries:
        entrypoint = e.get("entrypoint")
        specs.append(
            {
                "path": resolve_extension_path(
                    e["path"], base=base, resolve_relative=True
                ),
                "entrypoint": entrypoint if isinstance(entrypoint, str) else None,
            }
        )
    return specs
