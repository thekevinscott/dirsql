"""Subcommand entry point for `dirsql interpret <config>`.

Glues the per-purpose helpers (`load_app`, `dispatch_extract`,
`write_message`) into the long-running NDJSON loop:

  handshake (helper -> caller, once on startup):
    {"type": "config", "state": <vars(app)>}

  extract request (caller -> helper):
    {"type": "extract", "id": <int>, "table": "<name>", "path": "<abs>"}

  extract response (helper -> caller):
    {"type": "result", "id": <int>, "ok": true,  "rows": [...]}
    {"type": "result", "id": <int>, "ok": false, "error": "<msg>"}

Exits 0 when stdin closes; non-zero with a single ``dirsql interpret:``
line on stderr if the config can't be loaded.
"""

from __future__ import annotations

import json
import os
import sys

from ...resolve_config import INTERPRET_ROOT_ENV
from .dispatch_extract import dispatch_extract
from .load_app import load_app
from .write_message import write_message


def run(argv: list[str]) -> int:
    if len(argv) != 1:
        sys.stderr.write(
            f"dirsql interpret: expected one config path, got {len(argv)}\n"
        )
        return 1
    config_path = argv[0]

    # Expose the config file's parent directory as the implicit default root
    # for the user module we're about to import. A native config that omits
    # both `root` and `config` resolves its scan root to this value, matching
    # how a `.dirsql.toml` defaults its root (#251). Set before `load_app` so
    # the user's `DirSQL(...)` line sees it during construction.
    os.environ[INTERPRET_ROOT_ENV] = os.path.dirname(os.path.abspath(config_path))

    try:
        app = load_app(config_path)
    except Exception as exc:
        sys.stderr.write(f"dirsql interpret: {exc}\n")
        return 1

    # Name comes from `dirsql::db::parse_table_name` -- the canonical
    # core parser, surfaced via PyO3 on `Table.name` (#196). No regex
    # duplication on the Python side. Tables with a name the parser
    # couldn't extract are skipped here; the malformed DDL would also
    # surface as a `DirSqlError::Ddl` during the SDK scan, which
    # `await app.ready()` would re-raise -- not interpret's job to
    # second-guess.
    tables = {t.name: t for t in (app._tables or []) if t.name is not None}
    write_message({"type": "config", "state": vars(app)})

    for line in sys.stdin:
        stripped = line.strip()
        if not stripped:
            continue
        try:
            req = json.loads(stripped)
        except json.JSONDecodeError:
            continue
        if not isinstance(req, dict) or req.get("type") != "extract":
            continue
        write_message(dispatch_extract(req, tables))

    return 0
