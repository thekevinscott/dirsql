"""Colocated unit tests for the names a module-level statement binds (isolation
-- an `ast` node in, names out).
"""

import ast

from checks.platforms_mirror.assigned_names import assigned_names


def names(source: str) -> list[str]:
    return assigned_names(ast.parse(source).body[0])


def describe_assigned_names():
    def it_reads_an_annotated_assignment():
        assert names("PLATFORMS: tuple = ()\n") == ["PLATFORMS"]

    def it_reads_a_plain_assignment():
        assert names("PLATFORMS = ()\n") == ["PLATFORMS"]

    def it_reads_every_name_of_a_chained_assignment():
        assert names("OTHERS = PLATFORMS = ()\n") == ["OTHERS", "PLATFORMS"]

    def it_ignores_a_binding_that_is_not_a_plain_name():
        assert names("holder.PLATFORMS = ()\n") == []

    def it_binds_nothing_for_a_statement_that_assigns_nothing():
        assert names("import os\n") == []
