"""Single-line NDJSON writer for the `dirsql interpret` helper."""

from __future__ import annotations

import json
import sys
from typing import Any


def write_message(msg: dict[str, Any]) -> None:
    """Write one NDJSON line to stdout and flush.

    The flush is required so the orchestrator on the other side of the
    pipe sees the message immediately; without it the buffered stream
    would deliver lines in batches.
    """
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()
