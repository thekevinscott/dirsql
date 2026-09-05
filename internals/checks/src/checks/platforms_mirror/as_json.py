"""A comment-free object literal as JSON.

TypeScript's bare keys and trailing commas are the only two things left between
a stripped `PLATFORMS` array and something `json` will decode.
"""

from __future__ import annotations

import re


def as_json(text: str) -> str:
    """`text` with bare object keys quoted and trailing commas dropped."""
    keyed = re.sub(r"([{,]\s*)([A-Za-z_$][\w$]*)\s*:", r'\1"\2":', text)
    return re.sub(r",(\s*[}\]])", r"\1", keyed)
