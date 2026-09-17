"""Resolution of several TOML configs' ``[[dirsql.extension]]`` entries, in order.

The planning -- which configs parse, whether any entry names a package rather
than a file, and what each literal path resolves to against its own config's
directory -- lives in the Rust core (``_dirsql.plan_config_extensions``). This
module supplies the two host-specific halves the core cannot have: reading the
files, and locating an installed package with ``importlib``.
"""

from __future__ import annotations

import os

from ._dirsql import plan_config_extensions
from .plan_entry_path import _plan_entry_path
from .read_config import _read_config


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
    sources = [(os.path.abspath(p), _read_config(p)) for p in config_paths]
    plan = plan_config_extensions(sources)
    if plan is None:
        return None
    return [
        {"path": _plan_entry_path(entry), "entrypoint": entry.entrypoint}
        for entry in plan
    ]
