"""Colocated unit tests for the `[e2e]` table-to-flags mapping (#781)."""

from checks.preflight.e2e_flags import e2e_flags


def describe_e2e_flags():
    def it_maps_extra_scope_and_exclude_onto_repeatable_flags():
        assert e2e_flags({"extra_scope": ["a", "b"], "exclude": ["a/cli"]}) == [
            *["--extra-scope", "a", "--extra-scope", "b"],
            *["--exclude", "a/cli"],
        ]

    def it_returns_nothing_for_an_absent_table():
        assert e2e_flags({}) == []

    def it_keeps_extra_scope_ahead_of_exclude():
        assert e2e_flags({"exclude": ["x"], "extra_scope": ["y"]}) == [
            *["--extra-scope", "y"],
            *["--exclude", "x"],
        ]
