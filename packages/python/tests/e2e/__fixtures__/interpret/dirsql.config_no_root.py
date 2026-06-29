"""Fixture: a native Python config that omits ``root``.

`dirsql interpret` should resolve the root to the helper process's current
working directory (the cwd the orchestrator was launched from) rather than
erroring. Mirrors ``dirsql.config.py`` but drops the ``root=`` argument.
"""

import json

from dirsql import DirSQL, Table


def _extract(path):
    with open(path, encoding="utf-8") as f:
        return [json.load(f)]


app = DirSQL(
    tables=[
        Table(
            ddl="CREATE TABLE papers (title TEXT)",
            glob="**/meta.json",
            extract=_extract,
        )
    ],
)
