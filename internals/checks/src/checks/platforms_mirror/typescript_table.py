"""The `PLATFORMS` rows of a `platforms.ts`-shaped module.

No node toolchain and no evaluation: the source is stripped of comments,
normalized to JSON, and handed to `json`'s own decoder, which finds the end of
the array so nothing here counts brackets.
"""

from __future__ import annotations

import json
import re

from .as_json import as_json
from .parse import ParseError, TABLE_NAME
from .without_comments import without_comments


def typescript_table(source: str) -> list[dict]:
    """Rows of the `PLATFORMS` array in a `platforms.ts`-shaped module."""
    cleaned = without_comments(source)
    marker = re.search(rf"\b{TABLE_NAME}\b[^=;]*=\s*(\[)", cleaned)
    if marker is None:
        raise ParseError(f"no `{TABLE_NAME} = [...]` assignment in the TypeScript source.")
    try:
        # `raw_decode` stops at the end of the array, so nothing here has to find
        # the closing bracket or care what follows it.
        rows, _ = json.JSONDecoder().raw_decode(as_json(cleaned[marker.start(1) :]))
    except json.JSONDecodeError as error:
        raise ParseError(
            f"`{TABLE_NAME}` is not a plain array of object literals ({error}); this check "
            f"reads it as data and cannot evaluate spreads, computed keys or identifiers, "
            f"and cannot read an unterminated array, string or comment."
        ) from error
    if not all(isinstance(row, dict) for row in rows):
        raise ParseError(f"every `{TABLE_NAME}` entry must be an object literal.")
    return rows
