"""What both platform tables are called, and how an unreadable one fails (#1004).

Neither side is imported: `internals/checks` depends on neither
`internals/distcheck` nor a node toolchain, and a check that executes the file
it audits can be fooled by the file. `python_table.py` reads its side out of
`ast`; `typescript_table.py` strips comments, normalizes to JSON, and hands the
result to `json`'s own decoder.

Every shape those readers cannot read raises `ParseError` rather than returning
an empty table -- a mirror check that silently sees no rows passes forever.
"""

from __future__ import annotations

CLASS_NAME = "Platform"
TABLE_NAME = "PLATFORMS"


class ParseError(Exception):
    """A platform table that could not be read in the shape this check expects."""
