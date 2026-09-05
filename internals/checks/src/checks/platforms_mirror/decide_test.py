"""Colocated unit tests for the unmirrored-field verdict (isolation -- a field
list in, messages out).
"""

from checks.platforms_mirror.decide import SHARED, unmirrored_fields

FIELDS = list(SHARED)


def describe_unmirrored_fields():
    def it_passes_the_declared_subset():
        assert unmirrored_fields(FIELDS) == []

    def it_names_a_field_with_no_typescript_source():
        (message,) = unmirrored_fields([*FIELDS, "exe"])
        assert message.startswith("Platform.exe has no counterpart in ")
        assert "packages/ts/src/platforms.ts" in message

    def it_lists_the_subset_the_mirror_can_source():
        (message,) = unmirrored_fields(["exe"])
        assert "(node_platform, node_arch, slug, os, cpu)" in message

    def it_reports_every_unmirrored_field():
        assert len(unmirrored_fields(["exe", "dev"])) == 2
