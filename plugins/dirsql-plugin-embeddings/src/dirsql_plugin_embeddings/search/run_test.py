from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from . import run


def describe_config_fragment():
    def it_resolves_the_packaged_dirsql_toml():
        with patch.object(run, "resources") as resources:
            resources.files.return_value.joinpath.return_value = "frag.toml"
            assert run.config_fragment() == "frag.toml"
        resources.files.assert_called_once_with("dirsql_plugin_embeddings")
        resources.files.return_value.joinpath.assert_called_once_with(
            "dirsql.toml"
        )


def describe_run_search():
    def it_builds_the_sql_queries_the_sdk_and_formats_the_rows():
        rows = [{"path": "a", "distance": 0.5}]
        fake_dirsql = MagicMock()
        fake_dirsql.DirSQL.return_value.query = AsyncMock(return_value=rows)
        with patch.object(run, "build_search_sql", return_value="SQL") as build:
            with patch.object(run, "dirsql", fake_dirsql):
                with patch.object(run, "config_fragment", return_value="frag"):
                    with patch.object(
                        run, "format_rows", return_value=["line"]
                    ) as formatter:
                        assert run.run_search("g", "q", 5, "m") == ["line"]
        build.assert_called_once_with("g", "q", 5, "m")
        fake_dirsql.DirSQL.assert_called_once_with(config="frag")
        fake_dirsql.DirSQL.return_value.query.assert_awaited_once_with("SQL")
        formatter.assert_called_once_with(rows)

    def it_defaults_the_model_to_none():
        fake_dirsql = MagicMock()
        fake_dirsql.DirSQL.return_value.query = AsyncMock(return_value=[])
        with patch.object(run, "build_search_sql", return_value="SQL") as build:
            with patch.object(run, "dirsql", fake_dirsql):
                with patch.object(run, "config_fragment", return_value="frag"):
                    assert run.run_search("g", "q", 5) == []
        build.assert_called_once_with("g", "q", 5, None)


def sdk_returning(*results):
    fake_dirsql = MagicMock()
    fake_dirsql.DirSQL.return_value.query = AsyncMock(side_effect=results)
    return fake_dirsql


def search_with(fake_dirsql):
    with patch.object(run, "dirsql", fake_dirsql):
        with patch.object(run, "config_fragment", return_value="frag"):
            return run.run_search("docs/**/*.md", "q", 5)


def describe_empty_results():
    def it_raises_naming_the_glob_when_no_file_matched():
        with pytest.raises(run.NothingToRank) as raised:
            search_with(sdk_returning([], [{"n": 0}]))
        message = str(raised.value)
        assert "no files matched" in message
        assert "docs/**/*.md" in message
        assert "searched from" in message

    def it_says_so_when_files_matched_but_none_could_be_embedded():
        with pytest.raises(run.NothingToRank) as raised:
            search_with(sdk_returning([], [{"n": 3}]))
        message = str(raised.value)
        assert "3" in message
        assert "docs/**/*.md" in message
        assert "no files matched" not in message, (
            "files did match; the message must not claim otherwise"
        )

    def it_counts_the_corpus_only_when_the_ranking_is_empty():
        fake_dirsql = sdk_returning([{"path": "a", "distance": 0.5}])
        assert search_with(fake_dirsql) == ["a\t0.500000"]
        assert fake_dirsql.DirSQL.return_value.query.await_count == 1

    def it_reuses_one_sdk_instance_for_both_queries():
        fake_dirsql = sdk_returning([], [{"n": 0}])
        with pytest.raises(run.NothingToRank):
            search_with(fake_dirsql)
        assert fake_dirsql.DirSQL.call_count == 1
