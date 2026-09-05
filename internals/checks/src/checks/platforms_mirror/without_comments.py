"""A TypeScript source with its comments gone and its strings JSON-quoted.

Both happen in one pass because they cannot be separated: a `//` inside a
string is not a comment, and a quote inside a comment is not a string.
"""

from __future__ import annotations

import re

from .requoted import requoted

# One alternation of "things a naive scan would cut in half": the three string
# forms first, so a `//` inside a string is matched as string rather than as the
# comment that follows it.
_TOKENS = re.compile(
    r'"(?:\\.|[^"\\])*"' r"|'(?:\\.|[^'\\])*'" r"|`(?:\\.|[^`\\])*`" r"|//[^\n]*" r"|/\*.*?\*/",
    re.S,
)


def without_comments(source: str) -> str:
    def keep(match: re.Match) -> str:
        text = match.group(0)
        return "" if text.startswith(("//", "/*")) else requoted(text)

    return _TOKENS.sub(keep, source)
