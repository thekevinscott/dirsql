import asyncio
from importlib import resources

import dirsql

from .output import format_rows
from .sql import build_search_sql


def config_fragment():
    return str(resources.files("dirsql_plugin_embeddings").joinpath("dirsql.toml"))


async def _query(sql):
    return await dirsql.DirSQL(config=config_fragment()).query(sql)


def run_search(glob, query, limit, model=None):
    rows = asyncio.run(_query(build_search_sql(glob, query, limit, model)))
    return format_rows(rows)
