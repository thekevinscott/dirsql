"""SDK-side resolution of a TOML config's ``[[dirsql.extension]]`` entries.

The Rust core loads a config's extensions literally -- it has no
``importlib``, so it cannot resolve a bare **package name**. When a TOML
config names an extension by package name, the SDK resolves every one of its
extensions here, hands the core the resolved literal paths, and suppresses
the core's own config-extension loading (``suppress_config_extensions``) so
the config's entries are not loaded a second time.

Shared by the ``DirSQL`` constructor (``config=`` path) and the CLI launcher
(which converts the resolved specs into ``--extension`` flags), both of which
reach it through :mod:`dirsql.resolve_configs_extension_specs`.
"""

from __future__ import annotations

from .has_bare_name import _has_bare_name
from .load_extension_entries import _load_extension_entries
from .resolve_entries import _resolve_entries


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
    loaded = _load_extension_entries(config_path)
    if loaded is None:
        return None
    entries, base = loaded
    if not _has_bare_name(entries):
        return None
    return _resolve_entries(entries, base)
