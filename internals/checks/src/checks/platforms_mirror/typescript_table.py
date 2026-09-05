"""The `PLATFORMS` rows of a `platforms.ts`-shaped module.

No node toolchain and no evaluation: the source is stripped of comments,
normalized to JSON, and handed to `json`'s own decoder, which finds the end of
the array so nothing here counts brackets.
"""

from __future__ import annotations

import json
import re

from .parse import ParseError, TABLE_NAME

# One alternation of "things a naive scan would cut in half": the three string
# forms first, so a `//` inside a string is matched as string rather than as the
# comment that follows it.
_TOKENS = re.compile(
    r'"(?:\\.|[^"\\])*"' r"|'(?:\\.|[^'\\])*'" r"|`(?:\\.|[^`\\])*`" r"|//[^\n]*" r"|/\*.*?\*/",
    re.S,
)


def _requoted(text: str) -> str:
    """A TypeScript string literal as a JSON one."""
    if text.startswith('"'):
        return text
    return json.dumps(text[1:-1].replace('\\"', '"').replace("\\'", "'"))


def _without_comments(source: str) -> str:
    def keep(match: re.Match) -> str:
        text = match.group(0)
        return "" if text.startswith(("//", "/*")) else _requoted(text)

    return _TOKENS.sub(keep, source)


def _as_json(text: str) -> str:
    keyed = re.sub(r"([{,]\s*)([A-Za-z_$][\w$]*)\s*:", r'\1"\2":', text)
    return re.sub(r",(\s*[}\]])", r"\1", keyed)


def typescript_table(source: str) -> list[dict]:
    """Rows of the `PLATFORMS` array in a `platforms.ts`-shaped module."""
    cleaned = _without_comments(source)
    marker = re.search(rf"\b{TABLE_NAME}\b[^=;]*=\s*(\[)", cleaned)
    if marker is None:
        raise ParseError(f"no `{TABLE_NAME} = [...]` assignment in the TypeScript source.")
    try:
        # `raw_decode` stops at the end of the array, so nothing here has to find
        # the closing bracket or care what follows it.
        rows, _ = json.JSONDecoder().raw_decode(_as_json(cleaned[marker.start(1) :]))
    except json.JSONDecodeError as error:
        raise ParseError(
            f"`{TABLE_NAME}` is not a plain array of object literals ({error}); this check "
            f"reads it as data and cannot evaluate spreads, computed keys or identifiers, "
            f"and cannot read an unterminated array, string or comment."
        ) from error
    if not all(isinstance(row, dict) for row in rows):
        raise ParseError(f"every `{TABLE_NAME}` entry must be an object literal.")
    return rows
