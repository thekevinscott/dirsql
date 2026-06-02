"""`dirsql interpret <config>` -- long-running native config helper (#196).

Loads a Python config file, takes its module-level ``app = DirSQL(...)``,
and serves ``extract`` requests over NDJSON on stdin/stdout. One line in,
one line out, sequential. The Rust orchestrator spawns this process when
``--config`` points to a ``.py`` file.

Protocol (one JSON object per line, flushed on every write):

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

import importlib.util
import json
import os
import re
import sys
from typing import Any

# Pulls the table's SQL name out of `CREATE TABLE <name>` / `CREATE TABLE
# IF NOT EXISTS <name>` / quoted variants. The Python `Table` PyO3 class
# doesn't expose a `name` field, so the request dispatcher derives it
# from `ddl`.
_NAME_RE = re.compile(
    r'^\s*CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?["\'`]?(\w+)',
    re.IGNORECASE,
)


def _load_app(config_path: str) -> Any:
    abs_path = os.path.abspath(config_path)
    spec = importlib.util.spec_from_file_location("_dirsql_user_config", abs_path)
    if spec is None or spec.loader is None:
        raise ImportError(f"could not load config: {config_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    if not hasattr(module, "app"):
        raise AttributeError(
            f"{config_path}: module must define a top-level `app = DirSQL(...)`"
        )
    return module.app


def _table_name(ddl: str) -> str:
    m = _NAME_RE.match(ddl)
    if not m:
        raise ValueError(f"could not parse table name from DDL: {ddl!r}")
    return m.group(1)


def _write(msg: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


def run(argv: list[str]) -> int:
    if len(argv) != 1:
        sys.stderr.write(
            f"dirsql interpret: expected one config path, got {len(argv)}\n"
        )
        return 1
    config_path = argv[0]

    try:
        app = _load_app(config_path)
    except Exception as exc:
        sys.stderr.write(f"dirsql interpret: {exc}\n")
        return 1

    tables = {_table_name(t.ddl): t for t in (app._tables or [])}
    _write({"type": "config", "state": vars(app)})

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(req, dict) or req.get("type") != "extract":
            continue
        rid = req.get("id")
        name = req.get("table")
        path = req.get("path")
        table = tables.get(name)
        if table is None:
            _write(
                {
                    "type": "result",
                    "id": rid,
                    "ok": False,
                    "error": f"unknown table: {name!r}",
                }
            )
            continue
        try:
            rows = table.extract(path)
            _write({"type": "result", "id": rid, "ok": True, "rows": rows})
        except Exception as exc:
            _write(
                {"type": "result", "id": rid, "ok": False, "error": str(exc)}
            )

    return 0
