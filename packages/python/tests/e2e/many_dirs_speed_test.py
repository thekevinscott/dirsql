"""One-shot speed: a config table over a tree where every note has its own folder.

The one-shot query watches every directory under the table's anchor before it
walks, so a tree with as many folders as files is where that setup shows. The
notes, parser, question and natives are the frontmatter case's; only the shape
of the tree differs. No mocks: real console script, real process, real
filesystem, real parser spawn.
"""

from __future__ import annotations

import shutil

import pytest

from .frontmatter_parse_speed_test import (
    COLUMNS,
    CONFIG,
    FIELDS_AWK,
    NATIVE,
    NOTES_AWK,
    QUERY,
    STATUSES,
    native_rows,
)
from .speed_of_light import (
    AGG_AWK,
    MERGE_AWK,
    agreed_rows,
    assert_speed_of_light,
    baseline,
    cli,
    dirsql_rows,
    grow_until_native_takes_a_second,
    paired,
    shell_natives,
    startup_seconds,
    timed,
)


def build_folders(root, lo, hi):
    """Notes `lo` through `hi - 1`, each alone in its own folder, a thousand
    folders to a parent."""
    if lo == 0:
        root.mkdir(parents=True)
        (root / ".dirsql.toml").write_text(CONFIG)
        (root / "notes.awk").write_text(NOTES_AWK)
        (root / "fields.awk").write_text(FIELDS_AWK)
        (root / "agg.awk").write_text(AGG_AWK)
        (root / "merge.awk").write_text(MERGE_AWK)
    for i in range(lo, hi):
        path = root / "notes" / f"group-{i // 1000}" / f"note-{i}" / "note.md"
        path.parent.mkdir(parents=True)
        path.write_text(
            "---\n"
            f"status: {STATUSES[(i * 7) % len(STATUSES)]}\n"
            f"words: {i % 500}\n"
            "---\n"
        )


def describe_many_folders_speed_of_light():
    @pytest.fixture
    def root(tmp_path):
        tree = tmp_path / "vault"
        try:
            yield tree
        finally:
            shutil.rmtree(tree, ignore_errors=True)

    def it_matches_native_rows_within_the_bar(root):
        startup = startup_seconds()

        natives = shell_natives(root, NATIVE, native_rows, shell="sh")
        grow_until_native_takes_a_second(
            lambda lo, hi: build_folders(root, lo, hi),
            baseline(natives),
            start=2048,
            ceiling=2**17,
        )

        def dirsql(timeout):
            proc, seconds = timed(
                [cli(), "query", QUERY, "-c", ".dirsql.toml"],
                root,
                timeout=timeout,
            )
            return dirsql_rows(proc, COLUMNS), seconds

        result = paired(natives, dirsql, startup)
        expected, actual = agreed_rows(result), result.dirsql_rows
        assert len(expected) == len(STATUSES), expected

        assert actual == expected
        assert_speed_of_light("many folders", result, startup)
