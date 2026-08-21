"""Colocated unit tests for the platform-table readers (isolation -- pure text in,
data out; nothing here touches the repo's real platforms.py / platforms.ts).
"""

import pytest

from checks.platforms_mirror.parse import ParseError, python_table, typescript_table

PYTHON = '''\
from dataclasses import dataclass


@dataclass(frozen=True)
class Platform:
    node_platform: str
    node_arch: str
    slug: str


PLATFORMS: tuple[Platform, ...] = (
    Platform("linux", "x64", "linux-x64-gnu"),
    Platform("win32", "x64", slug="win32-x64-msvc", node_arch="x64"),
)
'''

TYPESCRIPT = """\
// Leading comment with a "quoted" brace { and a stray ].
export const PLATFORMS: readonly Platform[] = [
  {
    /* block comment */
    triple: "x86_64-unknown-linux-gnu",
    nodePlatform: "linux",
    os: ["linux"], // trailing comment
  },
];
"""


def describe_python_table():
    def it_reads_the_annotated_fields_and_the_rows():
        fields, rows = python_table(PYTHON)
        assert fields == ["node_platform", "node_arch", "slug"]
        assert rows[0] == {"node_platform": "linux", "node_arch": "x64", "slug": "linux-x64-gnu"}

    def it_lets_keywords_override_positionals():
        assert python_table(PYTHON)[1][1] == {
            "node_platform": "win32",
            "node_arch": "x64",
            "slug": "win32-x64-msvc",
        }

    def it_reads_a_plain_assignment_without_an_annotation():
        source = PYTHON.replace("PLATFORMS: tuple[Platform, ...] =", "PLATFORMS =")
        assert len(python_table(source)[1]) == 2

    def it_rejects_a_module_with_no_platform_class():
        with pytest.raises(ParseError, match="no `class Platform`"):
            python_table("PLATFORMS = ()\n")

    def it_rejects_a_platform_class_with_no_annotated_fields():
        with pytest.raises(ParseError, match="declares no annotated fields"):
            python_table("class Platform:\n    pass\n\n\nPLATFORMS = ()\n")

    def it_rejects_a_module_with_no_table():
        with pytest.raises(ParseError, match="no module-level"):
            python_table("class Platform:\n    slug: str\n")

    def it_rejects_a_computed_table():
        source = PYTHON.replace("PLATFORMS: tuple[Platform, ...] = (", "PLATFORMS = tuple(")
        with pytest.raises(ParseError, match="not a tuple or list literal"):
            python_table(source.replace(")\n", ")\n", 1))

    def it_rejects_a_row_that_is_not_a_platform_call():
        with pytest.raises(ParseError, match="literal `Platform"):
            python_table("class Platform:\n    slug: str\n\n\nPLATFORMS = (1,)\n")

    def it_rejects_a_row_with_more_positionals_than_fields():
        with pytest.raises(ParseError, match="positional arguments"):
            python_table('class Platform:\n    slug: str\n\n\nPLATFORMS = (Platform("a", "b"),)\n')

    def it_rejects_a_row_that_splats_keywords():
        with pytest.raises(ParseError, match="splats"):
            python_table("class Platform:\n    slug: str\n\n\nPLATFORMS = (Platform(**row),)\n")

    def it_rejects_a_non_literal_argument():
        with pytest.raises(ParseError, match="not a literal"):
            python_table("class Platform:\n    slug: str\n\n\nPLATFORMS = (Platform(slug),)\n")


def describe_typescript_table():
    def it_reads_the_array_through_comments_and_trailing_commas():
        assert typescript_table(TYPESCRIPT) == [
            {
                "triple": "x86_64-unknown-linux-gnu",
                "nodePlatform": "linux",
                "os": ["linux"],
            }
        ]

    def it_normalizes_single_quoted_strings():
        assert typescript_table("const PLATFORMS = [{ os: ['linux'] }];") == [{"os": ["linux"]}]

    def it_keeps_an_escape_sequence_inside_a_string():
        assert typescript_table(r'const PLATFORMS = [{ os: "a\"b" }];') == [{"os": 'a"b'}]

    def it_rejects_a_source_with_no_table():
        with pytest.raises(ParseError, match="no `PLATFORMS"):
            typescript_table("export const OTHER = [];\n")

    def it_rejects_an_unbalanced_array():
        with pytest.raises(ParseError, match="unbalanced"):
            typescript_table("const PLATFORMS = [{ os: [] }\n")

    def it_rejects_an_unterminated_block_comment():
        with pytest.raises(ParseError, match="unterminated block comment"):
            typescript_table("const PLATFORMS = [ /* open\n")

    def it_rejects_an_unterminated_string():
        with pytest.raises(ParseError, match="unterminated string"):
            typescript_table('const PLATFORMS = [{ os: "open\n')

    def it_rejects_an_unterminated_string_ending_in_a_backslash():
        with pytest.raises(ParseError, match="unterminated string"):
            typescript_table('const PLATFORMS = [{ os: "open\\')

    def it_rejects_a_line_comment_that_runs_to_the_end_of_the_file():
        with pytest.raises(ParseError, match="unbalanced"):
            typescript_table("const PLATFORMS = [ // open")

    def it_rejects_an_entry_that_is_not_an_object_literal():
        with pytest.raises(ParseError, match="object literal"):
            typescript_table('const PLATFORMS = ["linux"];')

    def it_rejects_a_table_it_cannot_read_as_data():
        with pytest.raises(ParseError, match="not a plain array"):
            typescript_table("const PLATFORMS = [{ ...spread }];")
