import asyncio
from unittest.mock import AsyncMock, MagicMock, patch

from . import search_rows as module


def sdk_returning(*results):
    fake_dirsql = MagicMock()
    fake_dirsql.DirSQL.return_value.query = AsyncMock(side_effect=results)
    return fake_dirsql


def search(fake_dirsql, glob="docs/**/*.md", model=None):
    with patch.object(module, "dirsql", fake_dirsql):
        with patch.object(module, "config_fragment", return_value="frag"):
            with patch.object(
                module, "build_search_sql", return_value="SQL"
            ) as build:
                with patch.object(
                    module, "count_corpus_sql", return_value="COUNT"
                ) as count:
                    rows = asyncio.run(module.search_rows(glob, "q", 5, model))
    return rows, build, count


def describe_config_fragment():
    def it_resolves_the_packaged_dirsql_toml():
        with patch.object(module, "resources") as resources:
            resources.files.return_value.joinpath.return_value = "frag.toml"
            assert module.config_fragment() == "frag.toml"
        resources.files.assert_called_once_with("dirsql_plugin_embeddings")
        resources.files.return_value.joinpath.assert_called_once_with(
            "dirsql.toml"
        )


def describe_search_rows():
    def it_queries_the_sdk_with_the_built_sql_and_the_packaged_fragment():
        fake_dirsql = sdk_returning([{"path": "a"}])
        rows, build, _ = search(fake_dirsql, model="m")
        assert rows == ([{"path": "a"}], None)
        build.assert_called_once_with("docs/**/*.md", "q", 5, "m")
        fake_dirsql.DirSQL.assert_called_once_with(config="frag")
        fake_dirsql.DirSQL.return_value.query.assert_awaited_once_with("SQL")

    def it_counts_the_corpus_only_when_the_ranking_is_empty():
        _, _, count = search(sdk_returning([{"path": "a"}]))
        count.assert_not_called()

    def it_returns_the_corpus_count_when_nothing_ranked():
        fake_dirsql = sdk_returning([], [{"n": 3}])
        rows, _, count = search(fake_dirsql)
        assert rows == ([], 3)
        count.assert_called_once_with("docs/**/*.md")
        assert fake_dirsql.DirSQL.return_value.query.await_count == 2

    def it_reuses_one_sdk_instance_for_both_queries():
        fake_dirsql = sdk_returning([], [{"n": 0}])
        search(fake_dirsql)
        assert fake_dirsql.DirSQL.call_count == 1
