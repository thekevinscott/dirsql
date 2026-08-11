"""Integration: the one-liner search CLI with the dirsql SDK mocked.

Drives the plugin's click entry point (`cli.main:main`) through click's
CliRunner with the SDK boundary (`DirSQL`) mocked per testing-conventions:
what is under test is the CLI's own contract -- bare positionals route to the
search command, the canonical search SQL is generated (with the query text,
model id, and glob safely escaped), the SDK is invoked with the packaged
config fragment, and ranked results print as path + distance lines.
"""

from unittest.mock import AsyncMock, patch

from click.testing import CliRunner

from dirsql_plugin_embeddings.cli.main import main

ROWS = [
    {"path": "notes/planet.txt", "distance": 0.0},
    {"path": "notes/greeting.txt", "distance": 1.0},
]


def invoke(args, rows=ROWS):
    with patch("dirsql_plugin_embeddings.search.run.DirSQL") as dirsql_class:
        dirsql_class.return_value.query = AsyncMock(return_value=rows)
        result = CliRunner().invoke(main, args)
    return result, dirsql_class


def queried_sql(dirsql_class):
    return dirsql_class.return_value.query.await_args.args[0]


def describe_default_command():
    def it_runs_the_canonical_search_sql_for_bare_positionals():
        result, dirsql_class = invoke(["docs/**/*.md", "local models"])
        assert result.exit_code == 0, result.output
        assert queried_sql(dirsql_class) == (
            "SELECT path,"
            " vec_distance_cosine(emb, embed('local models')) AS distance"
            " FROM (SELECT path, embed(content) AS emb FROM 'docs/**/*.md')"
            " ORDER BY distance LIMIT 10"
        )

    def it_passes_the_packaged_config_fragment_to_the_sdk():
        result, dirsql_class = invoke(["docs/**/*.md", "local models"])
        assert result.exit_code == 0, result.output
        (config,) = [dirsql_class.call_args.kwargs["config"]]
        assert config.endswith("dirsql.toml")

    def it_prints_one_path_and_distance_line_per_row_in_rank_order():
        result, _ = invoke(["docs/**/*.md", "local models"])
        assert result.output.splitlines() == [
            "notes/planet.txt\t0.000000",
            "notes/greeting.txt\t1.000000",
        ]

    def it_templates_the_model_id_into_both_embed_calls():
        result, dirsql_class = invoke(
            ["docs/**/*.md", "local models", "--model", "my/model"]
        )
        assert result.exit_code == 0, result.output
        assert queried_sql(dirsql_class) == (
            "SELECT path,"
            " vec_distance_cosine(emb, embed('local models', 'my/model'))"
            " AS distance"
            " FROM (SELECT path, embed(content, 'my/model') AS emb"
            " FROM 'docs/**/*.md')"
            " ORDER BY distance LIMIT 10"
        )

    def it_uses_k_as_the_sql_limit():
        result, dirsql_class = invoke(["docs/**/*.md", "local models", "-k", "3"])
        assert result.exit_code == 0, result.output
        assert queried_sql(dirsql_class).endswith("ORDER BY distance LIMIT 3")

    def it_accepts_the_long_limit_spelling():
        result, dirsql_class = invoke(
            ["docs/**/*.md", "local models", "--limit", "5"]
        )
        assert result.exit_code == 0, result.output
        assert queried_sql(dirsql_class).endswith("ORDER BY distance LIMIT 5")

    def it_works_as_the_explicit_search_subcommand_too():
        result, dirsql_class = invoke(["search", "docs/**/*.md", "local models"])
        assert result.exit_code == 0, result.output
        assert "'docs/**/*.md'" in queried_sql(dirsql_class)


def describe_escaping():
    def it_escapes_single_quotes_in_the_query_text():
        result, dirsql_class = invoke(["docs/**/*.md", "it's a 'test'"])
        assert result.exit_code == 0, result.output
        assert "embed('it''s a ''test''')" in queried_sql(dirsql_class)

    def it_keeps_an_injection_shaped_query_inside_the_literal():
        result, dirsql_class = invoke(["docs/**/*.md", "'; DROP TABLE x; --"])
        assert result.exit_code == 0, result.output
        assert "embed('''; DROP TABLE x; --')" in queried_sql(dirsql_class)

    def it_escapes_single_quotes_in_the_model_id():
        result, dirsql_class = invoke(
            ["docs/**/*.md", "q", "--model", "it's"]
        )
        assert result.exit_code == 0, result.output
        assert "embed('q', 'it''s')" in queried_sql(dirsql_class)

    def it_escapes_single_quotes_in_the_glob():
        result, dirsql_class = invoke(["it's/**", "q"])
        assert result.exit_code == 0, result.output
        assert "FROM 'it''s/**'" in queried_sql(dirsql_class)


def describe_argument_errors():
    def it_errors_actionably_when_the_query_is_missing():
        result = CliRunner().invoke(main, ["docs/**/*.md"])
        assert result.exit_code == 2
        assert "Missing argument" in result.stderr
        assert "QUERY" in result.stderr
        assert "Traceback" not in result.stderr

    def it_errors_actionably_when_run_with_no_arguments():
        result = CliRunner().invoke(main, [])
        assert result.exit_code == 2
        assert "Missing argument" in result.stderr
        assert "GLOB" in result.stderr
        assert "Traceback" not in result.stderr

    def it_still_dispatches_the_worker_subcommand():
        from dirsql_plugin_embeddings.cli.main import main as group

        assert "worker" in group.commands

    def it_shows_help_listing_both_commands():
        result = CliRunner().invoke(main, ["--help"])
        assert result.exit_code == 0
        assert "worker" in result.output
        assert "search" in result.output
