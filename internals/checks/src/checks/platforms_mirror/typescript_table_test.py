"""Colocated unit tests for the TypeScript-table reader (isolation -- pure text
in, data out; nothing here touches the repo's real platforms.ts).
"""

import pytest

from checks.platforms_mirror.typescript_table import ParseError, typescript_table

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

    def it_finds_the_array_when_a_comment_mentions_a_bracket():
        source = "// see PLATFORMS = [ elsewhere\nconst PLATFORMS = [{ os: ['a'] }];"
        assert typescript_table(source) == [{"os": ["a"]}]

    def it_reads_the_first_platforms_binding_it_finds():
        source = "const OTHER = [];\nexport const PLATFORMS = [{ os: ['a'] }];"
        assert typescript_table(source) == [{"os": ["a"]}]

    def it_rejects_a_source_with_no_table():
        with pytest.raises(ParseError, match="no `PLATFORMS"):
            typescript_table("export const OTHER = [];\n")

    @pytest.mark.parametrize(
        "source",
        [
            "const PLATFORMS = [{ os: [] }\n",
            "const PLATFORMS = [ /* open\n",
            'const PLATFORMS = [{ os: "open\n',
            'const PLATFORMS = [{ os: "open\\',
            "const PLATFORMS = [ // open",
        ],
        ids=["unclosed-array", "unclosed-block-comment", "unclosed-string", "trailing-backslash", "comment-to-eof"],
    )
    def it_rejects_a_table_it_cannot_read_to_the_end(source):
        with pytest.raises(ParseError, match="not a plain array"):
            typescript_table(source)

    def it_rejects_an_entry_that_is_not_an_object_literal():
        with pytest.raises(ParseError, match="object literal"):
            typescript_table('const PLATFORMS = ["linux"];')

    def it_rejects_a_table_it_cannot_read_as_data():
        with pytest.raises(ParseError, match="not a plain array"):
            typescript_table("const PLATFORMS = [{ ...spread }];")
