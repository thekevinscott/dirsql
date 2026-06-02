"""Happy-path fixture for `dirsql interpret` integration tests.

Exposes a single `papers` table whose `extract` reads each `meta.json`
under the colocated `data/` tree and returns its parsed contents.
"""

import json
import os

from dirsql import DirSQL, Table


def _extract(path):
    with open(path, encoding="utf-8") as f:
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
