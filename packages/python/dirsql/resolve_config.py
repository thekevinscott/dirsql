"""Synchronous config resolver for `DirSQL.__dict__`.

Mirrors `DirSQLBuilder::resolve` in the Rust core: explicit kwargs win for
scalars; tables and ignore lists are concatenated; persist is OR-ed;
path-valued config fields resolve relative to the config file's parent.
"""

import os
import tomllib

# Environment variable that the `dirsql interpret` launcher sets, before
# importing a user's native config module, to the config file's parent
# directory. A native config that supplies neither `root` nor `config`
# defaults its scan root to this value -- matching how a `.dirsql.toml`
# defaults its root to the config's parent directory (#251). Outside the
# interpret subprocess the variable is unset, so normal SDK use is
# unaffected.
INTERPRET_ROOT_ENV = "DIRSQL_INTERPRET_ROOT"


def resolve_config(root, tables, ignore, config, persist, persist_path):
    """Merge kwargs with a `.dirsql.toml` into the serialized state shape."""
    cfg, cfg_tables, cfg_dir = {}, [], None
    if config is not None:
        with open(config, "rb") as f:
            doc = tomllib.load(f)
        cfg = doc.get("dirsql") or {}
        cfg_tables = doc.get("table") or []
        cfg_dir = os.path.dirname(os.path.abspath(config))

    def _abs(p):
        return p if os.path.isabs(p) else os.path.join(cfg_dir, p)  # ty:ignore[no-matching-overload]

    # Precedence for `root`: explicit kwarg > config-derived (`[dirsql].root`
    # or the config's parent dir) > the interpret launcher's implicit root.
    # The implicit root only applies when neither a root nor a config was
    # given -- i.e. a native config with no `root` (#251).
    resolved_root = root or (
        (_abs(cfg["root"]) if "root" in cfg else cfg_dir)
        if config is not None
        else os.environ.get(INTERPRET_ROOT_ENV)
    )

    return {
        "root": resolved_root,
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
    }
