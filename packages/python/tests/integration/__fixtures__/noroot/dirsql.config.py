"""A native Python config file with no explicit ``root`` (issue #251).

When ``dirsql --config <path-to-this-file>`` runs, the Rust binary
spawns ``dirsql interpret`` against this file. With ``root`` omitted,
the scan root must default to this config file's parent directory --
exactly as a ``.dirsql.toml`` does -- so the ``data/**/meta.json``
files below are indexed. Before the #251 fix, ``DirSQL(tables=[...])``
raised "requires either a root directory or a config", the interpret
child died before the handshake, and the server returned HTTP 503.
"""

import json

from dirsql import DirSQL, Table


def _extract(path):
    with open(path) as f:
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
