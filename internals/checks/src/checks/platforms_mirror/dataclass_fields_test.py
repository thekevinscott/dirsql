"""Colocated unit tests for the mirrored field list (isolation -- a parsed
module in, field names out; nothing here touches the repo's real platforms.py).
"""

import ast

import pytest

from checks.platforms_mirror.dataclass_fields import ParseError, dataclass_fields


def fields(source: str) -> list[str]:
    return dataclass_fields(ast.parse(source))


def describe_dataclass_fields():
    def it_reads_the_annotations_in_declaration_order():
        source = "class Platform:\n    node_platform: str\n    node_arch: str\n    slug: str\n"
        assert fields(source) == ["node_platform", "node_arch", "slug"]

    def it_ignores_a_member_with_no_annotation():
        source = "class Platform:\n    slug: str\n    EXTRA = 1\n"
        assert fields(source) == ["slug"]

    @pytest.mark.parametrize("name", ["Machine", "Target"])
    def it_rejects_a_module_whose_only_class_is_named_otherwise(name):
        with pytest.raises(ParseError, match="no `class Platform`"):
            fields(f"class {name}:\n    slug: str\n")

    def it_rejects_a_module_with_no_class_at_all():
        with pytest.raises(ParseError, match="no `class Platform`"):
            fields("PLATFORMS = ()\n")

    def it_rejects_a_platform_class_with_no_annotated_fields():
        with pytest.raises(ParseError, match="declares no annotated fields"):
            fields("class Platform:\n    pass\n")
