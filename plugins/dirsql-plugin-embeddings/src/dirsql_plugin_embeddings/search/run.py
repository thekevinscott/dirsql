import asyncio
import os
from importlib import resources

import dirsql

from .output import format_rows
from .sql import build_search_sql, count_corpus_sql


class NothingToRank(Exception):
    """The search produced no rows, and why -- so the CLI can say which."""


def config_fragment():
    return str(resources.files("dirsql_plugin_embeddings").joinpath("dirsql.toml"))


def no_rows_message(glob, matched, root):
    if not matched:
        return f"no files matched {glob!r} (searched from {root})"
    return (
        f"{glob!r} matched {matched} file(s), but none had text content to"
        f" embed -- unreadable or not valid UTF-8 (searched from {root})"
    )


async def _search(glob, query, limit, model):
    db = dirsql.DirSQL(config=config_fragment())
    rows = await db.query(build_search_sql(glob, query, limit, model))
    if rows:
        return rows, None
    # Only now, on the error path, is the second scan worth its cost: it is
    # what tells "no files matched" apart from "matched, none embeddable".
    # One row, always: `SELECT COUNT(*)` cannot return anything else, and
    # unpacking says so rather than trusting an index.
    (counted,) = await db.query(count_corpus_sql(glob))
    return rows, counted["n"]


def run_search(glob, query, limit, model=None):
    rows, matched = asyncio.run(_search(glob, query, limit, model))
    if not rows:
        raise NothingToRank(no_rows_message(glob, matched, os.getcwd()))
    return format_rows(rows)
