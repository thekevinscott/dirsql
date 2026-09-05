"""The `Platform` fields and `PLATFORMS` rows of a `platforms.py`-shaped module.

The field list comes off the dataclass annotations and the rows off the
module-level assignment, both through `ast`: the module is never imported, so a
table that builds itself at import time is a `ParseError` rather than a read.
"""

from __future__ import annotations

import ast

from .dataclass_fields import dataclass_fields
from .row import row
from .table_elements import table_elements


def python_table(source: str) -> tuple[list[str], list[dict]]:
    """`(field names, rows)` from a `platforms.py`-shaped module."""
    tree = ast.parse(source)
    fields = dataclass_fields(tree)
    return fields, [row(element, fields) for element in table_elements(tree)]
