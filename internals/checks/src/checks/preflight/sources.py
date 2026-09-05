"""Resolve which workflows the gate matrix is derived from (#781).

Which workflows hold the reusable-workflow callers is itself discovered rather
than named. The matrix used to come from `conventions.yml` alone; #834 split it
into six per-domain workflows and deleted it, so the named default resolved to
nothing and `just preflight` died on a bare `FileNotFoundError` (#973). A list of
six names would only move the same failure one rename away.
"""

from __future__ import annotations

import os
from collections.abc import Callable, Sequence

from .discovery import discovered
from .matrix import WORKFLOWS
from .named_workflow import named
from .read_text import read_text


def sources(
    conventions: Sequence[str],
    *,
    directory: str = WORKFLOWS,
    listdir: Callable[[str], list[str]] = os.listdir,
    read: Callable[[str], str] = read_text,
) -> list[tuple[str, str]]:
    """(path, text) for the workflows the gate matrix is derived from."""
    if conventions:
        return [(path, named(path, read)) for path in conventions]
    return discovered(directory, listdir, read)
