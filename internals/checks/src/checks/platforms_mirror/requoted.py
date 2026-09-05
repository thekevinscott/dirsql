"""A TypeScript string literal as a JSON one.

Single-quoted and template forms carry the same characters JSON spells with
double quotes, so re-quoting is what lets `json` read the table as data.
"""

from __future__ import annotations

import json


def requoted(text: str) -> str:
    """A TypeScript string literal as a JSON one."""
    if text.startswith('"'):
        return text
    return json.dumps(text[1:-1].replace('\\"', '"').replace("\\'", "'"))
