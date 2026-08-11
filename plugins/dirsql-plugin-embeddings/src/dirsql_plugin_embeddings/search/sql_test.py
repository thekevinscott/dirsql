from . import sql


def describe_quote():
    def it_wraps_text_in_single_quotes():
        assert sql.quote("hello") == "'hello'"

    def it_doubles_embedded_single_quotes():
        assert sql.quote("O'Brien") == "'O''Brien'"

    def it_keeps_an_injection_shaped_value_inside_the_literal():
        assert sql.quote("'; DROP TABLE x; --") == "'''; DROP TABLE x; --'"


def describe_normalize_glob():
    def it_prefixes_a_bare_relative_glob_with_dot_slash():
        assert sql.normalize_glob("**/*.md") == "./**/*.md"

    def it_keeps_a_dot_slash_glob():
        assert sql.normalize_glob("./notes/*.txt") == "./notes/*.txt"

    def it_keeps_a_parent_relative_glob():
        assert sql.normalize_glob("../notes/*.txt") == "../notes/*.txt"

    def it_keeps_an_absolute_glob():
        assert sql.normalize_glob("/tmp/notes/*.txt") == "/tmp/notes/*.txt"

    def it_keeps_a_home_glob():
        assert sql.normalize_glob("~/notes/*.txt") == "~/notes/*.txt"


def describe_embed_call():
    def it_omits_the_model_argument_when_model_is_none():
        assert sql.embed_call("content", None) == "embed(content)"

    def it_quotes_the_model_id_as_the_second_argument():
        assert sql.embed_call("content", "my/model") == (
            "embed(content, 'my/model')"
        )

    def it_escapes_quotes_in_the_model_id():
        assert sql.embed_call("content", "it's") == "embed(content, 'it''s')"


def describe_build_search_sql():
    def it_builds_the_canonical_sql_without_a_model():
        assert sql.build_search_sql("./docs/**/*.md", "local models", 10) == (
            "SELECT path,"
            " vec_distance_cosine(emb, embed('local models')) AS distance"
            " FROM (SELECT path, embed(content) AS emb FROM './docs/**/*.md')"
            " ORDER BY distance LIMIT 10"
        )

    def it_templates_the_model_into_both_embed_calls():
        assert sql.build_search_sql("./docs/**/*.md", "q", 3, "my/model") == (
            "SELECT path,"
            " vec_distance_cosine(emb, embed('q', 'my/model')) AS distance"
            " FROM (SELECT path, embed(content, 'my/model') AS emb"
            " FROM './docs/**/*.md')"
            " ORDER BY distance LIMIT 3"
        )

    def it_normalizes_a_bare_glob_into_the_from_clause():
        built = sql.build_search_sql("docs/**/*.md", "q", 10)
        assert "FROM './docs/**/*.md'" in built

    def it_escapes_quotes_in_query_and_glob():
        built = sql.build_search_sql("it's/**", "it's a 'test'", 10)
        assert "embed('it''s a ''test''')" in built
        assert "FROM './it''s/**'" in built

    def it_defaults_the_model_to_none():
        assert "embed(content)" in sql.build_search_sql("g", "q", 10)

    def it_coerces_the_limit_to_a_decimal_integer():
        assert sql.build_search_sql("g", "q", "12").endswith("LIMIT 12")
