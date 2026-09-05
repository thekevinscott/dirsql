"""The default reader for the platforms-mirror check (#1004).

Split from the gate so the orchestration holds no I/O: the gate takes this as
an injected default and unit-tests against text rather than the repo's files.
"""

from __future__ import annotations


def read(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()
