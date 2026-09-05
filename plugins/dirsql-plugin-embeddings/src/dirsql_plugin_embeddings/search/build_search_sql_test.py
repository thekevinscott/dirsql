from unittest.mock import patch

from . import build_search_sql as module


def build(glob, query, limit, *args):
    with patch.object(module, "quote", side_effect=lambda text: f"<{text}>"):
        with patch.object(
            module, "normalize_glob", side_effect=lambda g: f"N({g})"
        ):
            with patch.object(
                module, "embed_call", side_effect=lambda a, m: f"E({a},{m})"
            ) as embed:
                return module.build_search_sql(glob, query, limit, *args), embed


def describe_build_search_sql():
    def it_assembles_the_ranking_query_from_its_collaborators():
        sql, _ = build("docs/**/*.md", "local models", 10, "my/model")
        assert sql == (
            "SELECT path, vec_distance_cosine(emb, E(<local models>,my/model))"
            " AS distance"
            " FROM (SELECT path, E(content,my/model) AS emb"
            " FROM <N(docs/**/*.md)>)"
            " WHERE emb IS NOT NULL"
            " ORDER BY distance LIMIT 10"
        )

    def it_embeds_the_query_and_the_content_column_with_the_same_model():
        _, embed = build("g", "q", 10, "my/model")
        assert [args for args, _ in embed.call_args_list] == [
            ("<q>", "my/model"),
            ("content", "my/model"),
        ]

    def it_defaults_the_model_to_none():
        _, embed = build("g", "q", 10)
        assert [args[1] for args, _ in embed.call_args_list] == [None, None]

    def it_drops_rows_whose_embedding_is_null():
        # A file that is unreadable or not valid UTF-8 has NULL content, so
        # embed() returns NULL and vec_distance_cosine() does too. SQLite
        # sorts NULLs first ascending, so without this guard those files take
        # the top-k slots and then break the distance formatting.
        sql, _ = build("g", "q", 10)
        assert " WHERE emb IS NOT NULL ORDER BY distance" in sql

    def it_coerces_the_limit_to_a_decimal_integer():
        sql, _ = build("g", "q", "12")
        assert sql.endswith("LIMIT 12")
