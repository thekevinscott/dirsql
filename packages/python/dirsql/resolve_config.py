"""Synchronous config resolver for `DirSQL.__dict__`.

Mirrors `DirSQLBuilder::resolve` in the Rust core: explicit kwargs win for
scalars; tables and ignore lists are concatenated; persist is OR-ed;
path-valued config fields resolve relative to the config file's parent.
"""

import os
import tomllib


def resolve_config(
    root, tables, ignore, config, persist, persist_path, extensions=None
):
    """Merge kwargs with a `.dirsql.toml` into the serialized state shape."""
    cfg, cfg_tables, cfg_extensions, cfg_dir = {}, [], [], None
    if config is not None:
        with open(config, "rb") as f:
            doc = tomllib.load(f)
        cfg = doc.get("dirsql") or {}
        cfg_tables = doc.get("table") or []
        cfg_extensions = cfg.get("extension") or []
        cfg_dir = os.path.dirname(os.path.abspath(config))

    def _abs(p):
        # Only reached from the `"<key>" in cfg` branches below, and cfg is
        # non-empty only after a config file was loaded -- which sets cfg_dir.
        base = cfg_dir
        assert base is not None
        return p if os.path.isabs(p) else os.path.join(base, p)

    return {
        "root": root or (_abs(cfg["root"]) if "root" in cfg else cfg_dir),
        "tables": [
            {"ddl": t.ddl, "glob": t.glob, "strict": bool(t.strict)}
            for t in (tables or [])
        ]
        + [
            {
                "ddl": e.get("ddl"),
                "glob": e.get("glob"),
                "strict": bool(e.get("strict")),
            }
            for e in cfg_tables
        ],
        "ignore": list(ignore or []) + list(cfg.get("ignore") or []),
        "persist": bool(persist or cfg.get("persist")),
        "persist_path": persist_path
        or (_abs(cfg["persist_path"]) if "persist_path" in cfg else None),
        # Programmatic extensions first (verbatim paths, mirroring the Rust
        # builder), then config-file `[[dirsql.extension]]` entries with
        # relative paths resolved against the config's parent directory.
        "extensions": [
            {"path": e["path"], "entrypoint": e.get("entrypoint")}
            for e in (extensions or [])
        ]
        + [
            {"path": _abs(e["path"]), "entrypoint": e.get("entrypoint")}
            for e in cfg_extensions
        ],
    }
