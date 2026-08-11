from unittest.mock import AsyncMock, MagicMock, patch

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
