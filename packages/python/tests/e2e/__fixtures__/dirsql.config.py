"""A native Python config file for dirsql.

When ``dirsql --config <path-to-this-file>`` runs, the Rust binary
spawns ``dirsql interpret`` against this file. The helper loads the
module, takes its module-level ``app``, and dispatches ``extract``
callbacks over NDJSON.
"""

import json
import os

from dirsql import DirSQL, Table


def _extract(path):
    with open(path) as f:
        return [json.load(f)]


app = DirSQL(
    root=os.path.join(os.path.dirname(__file__), "data"),
    tables=[
        Table(
            ddl="CREATE TABLE papers (title TEXT)",
            glob="**/meta.json",
            extract=_extract,
        )
    ],
)
