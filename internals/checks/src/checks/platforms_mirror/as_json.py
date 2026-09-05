"""An object-literal source as JSON: keys quoted, trailing commas dropped."""

from __future__ import annotations

import re


def as_json(text: str) -> str:
    keyed = re.sub(r"([{,]\s*)([A-Za-z_$][\w$]*)\s*:", r'\1"\2":', text)
    return re.sub(r",(\s*[}\]])", r"\1", keyed)
