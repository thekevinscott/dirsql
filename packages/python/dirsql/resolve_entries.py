"""Resolution of one config's ``[[dirsql.extension]]`` entries to literal paths."""

from __future__ import annotations

from .resolve_extension import resolve_extension_path


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
