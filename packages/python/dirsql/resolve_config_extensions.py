"""SDK-side resolution of a TOML config's ``[[dirsql.extension]]`` entries.

The Rust core loads a config's extensions literally -- it has no
``importlib``, so it cannot resolve a bare **package name**. When a TOML
config names an extension by package name, the SDK resolves every one of its
extensions here, hands the core the resolved literal paths, and suppresses
the core's own config-extension loading (``suppress_config_extensions``) so
the config's entries are not loaded a second time.

Shared by the ``DirSQL`` constructor (``config=`` path) and the CLI launcher
(which converts the resolved specs into ``--extension`` flags).
"""

from __future__ import annotations

import os
import sys
from importlib import import_module

from .resolve_extension import is_bare_name, resolve_extension_path


def _load_toml_module():
    """Return the TOML parser module for the running interpreter.

    ``tomllib`` is stdlib only on 3.11+; on 3.10 the ``tomli`` backport it was
    derived from (a version-gated dependency) provides the same surface.
    Imported by name so a unit test can exercise both arms on any
    interpreter -- a literal ``import tomllib`` is unreachable on 3.10 no
    matter what ``sys.version_info`` claims.
    """
    if sys.version_info >= (3, 11):
        return import_module("tomllib")
    return import_module("tomli")


_toml = _load_toml_module()


def _load_extension_entries(config_path):
    """Return ``(entries, base_dir)`` for a config's ``[[dirsql.extension]]``.

    ``None`` when the config is missing, unreadable/malformed, or declares no
    extension array -- the caller should leave such configs to the core.
    """
    if not os.path.isfile(config_path):
        return None
    try:
        with open(config_path, "rb") as f:
            doc = _toml.load(f)
    except (OSError, _toml.TOMLDecodeError):
        # Leave a malformed / unreadable config for the core to report.
        return None

    entries = (doc.get("dirsql") or {}).get("extension")
    if not isinstance(entries, list):
        return None
    return entries, os.path.dirname(os.path.abspath(config_path))


def _has_bare_name(entries):
    return any(
        isinstance(e, dict)
        and isinstance(e.get("path"), str)
        and is_bare_name(e["path"])
        for e in entries
    )


def _resolve_entries(entries, base):
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


def resolve_configs_extension_specs(config_paths):
    """Resolve the ``[[dirsql.extension]]`` entries of several configs, in order.

    The SDK intervenes for the whole set only when **some** config names an
    extension by bare package name (the core can resolve neither package names
    nor -- once globally suppressed -- the literal entries of the other
    configs). When it intervenes it resolves **every** config's entries, each
    against that config's own parent directory, concatenated in ``config_paths``
    order; the caller suppresses the core's config-extension loading and passes
    the resolved list. Returns ``None`` when no config uses a package name,
    leaving every config's loading to the core.
    """
    loaded = [_load_extension_entries(p) for p in config_paths]
    if not any(item is not None and _has_bare_name(item[0]) for item in loaded):
        return None
    specs = []
    for item in loaded:
        if item is None:
            continue
        entries, base = item
        specs.extend(_resolve_entries(entries, base))
    return specs
