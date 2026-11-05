"""Unit tests for `insert_flags`."""

from .insert_flags import insert_flags


def describe_insert_flags():
    def it_appends_when_argv_has_no_end_of_options_marker():
        assert insert_flags(["query", "x"], ["-c", "f"]) == ["query", "x", "-c", "f"]

    def it_inserts_before_the_first_end_of_options_marker():
        assert insert_flags(["--format", "json", "--", "x", "--"], ["-c", "f"]) == [
            "--format",
            "json",
            "-c",
            "f",
            "--",
            "x",
            "--",
        ]

    def it_inserts_before_a_leading_marker():
        assert insert_flags(["--", "x"], ["-c", "f"]) == ["-c", "f", "--", "x"]
