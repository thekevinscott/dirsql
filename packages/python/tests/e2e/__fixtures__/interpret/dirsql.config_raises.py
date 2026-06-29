"""Fixture whose `extract` raises -- exercises the `ok: false` response."""

import os

from dirsql import DirSQL, Table


def _boom(_path):
    raise ValueError("synthetic extract failure")


app = DirSQL(
    root=os.path.join(os.path.dirname(__file__), "data"),
    tables=[
        Table(
            ddl="CREATE TABLE papers (title TEXT)",
            glob="**/meta.json",
            extract=_boom,
        )
    ],
)
