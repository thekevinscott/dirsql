"""Resolution of several TOML configs' ``[[dirsql.extension]]`` entries, in order.

The plural counterpart to :func:`dirsql.resolve_config_extensions
.resolve_config_extension_specs`, used by the SDK constructor and the CLI
launcher when argv carries more than one ``--config``.
"""

from __future__ import annotations

from .has_bare_name import _has_bare_name
from .load_extension_entries import _load_extension_entries
from .resolve_entries import _resolve_entries


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
