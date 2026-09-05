import asyncio
import os

from .no_rows_message import no_rows_message
from .output import format_rows
from .search_rows import search_rows


class NothingToRank(Exception):
    """The search produced no rows, and why -- so the CLI can say which."""


def run_search(glob, query, limit, model=None):
    rows, matched = asyncio.run(search_rows(glob, query, limit, model))
    if not rows:
        raise NothingToRank(no_rows_message(glob, matched, os.getcwd()))
    return format_rows(rows)
