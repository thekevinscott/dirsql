"""Colocated unit test for `build_rows`."""

from .build_rows import build_rows


def describe_build_rows():
    def it_builds_one_row_with_the_embedding_as_json_text():
        assert build_rows("/a/b.md", "hello", [1.0, 2.0]) == [
            {"path": "/a/b.md", "text": "hello", "embedding": "[1.0, 2.0]"}
        ]

    def it_serializes_an_empty_vector_rather_than_dropping_the_column():
        assert build_rows("/a/b.md", "", []) == [
            {"path": "/a/b.md", "text": "", "embedding": "[]"}
        ]
