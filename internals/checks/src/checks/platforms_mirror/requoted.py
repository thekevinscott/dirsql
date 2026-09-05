"""A TypeScript string literal as a JSON one.

Single-quoted and template forms are unquoted by hand and re-encoded, so the
result is something `json` will accept without the escapes changing meaning.
"""

from __future__ import annotations

import json


def requoted(text: str) -> str:
    """A TypeScript string literal as a JSON one."""
    if text.startswith('"'):
        return text
    return json.dumps(text[1:-1].replace('\\"', '"').replace("\\'", "'"))
