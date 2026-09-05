"""A `platforms.ts`-shaped source with its comments stripped and strings requoted.

One alternation of "things a naive scan would cut in half": the three string
forms come first, so a `//` inside a string is matched as string rather than as
the comment that follows it.
"""

from __future__ import annotations

import re

from .requoted import requoted

_TOKENS = re.compile(
    r'"(?:\\.|[^"\\])*"' r"|'(?:\\.|[^'\\])*'" r"|`(?:\\.|[^`\\])*`" r"|//[^\n]*" r"|/\*.*?\*/",
    re.S,
)


def without_comments(source: str) -> str:
    """`source` with comments removed and every string literal JSON-quoted."""

    def keep(match: re.Match) -> str:
        text = match.group(0)
        return "" if text.startswith(("//", "/*")) else requoted(text)

    return _TOKENS.sub(keep, source)
