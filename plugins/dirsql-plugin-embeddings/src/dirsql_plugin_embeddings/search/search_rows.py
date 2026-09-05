from importlib import resources

import dirsql

from .build_search_sql import build_search_sql
from .count_corpus_sql import count_corpus_sql


def config_fragment():
    return str(resources.files("dirsql_plugin_embeddings").joinpath("dirsql.toml"))


async def search_rows(glob, query, limit, model):
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
