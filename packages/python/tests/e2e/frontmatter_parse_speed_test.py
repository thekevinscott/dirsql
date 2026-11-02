"""Parse speed: many markdown files with frontmatter.

A tree of notes, each opening with a `---` frontmatter block, parsed by one
awk process named in `.dirsql.toml`; the question is how many notes carry each
status and their summed word counts. Natives run the same awk over every file,
found by `find` or `fd`, in one process or in `xargs -P` batches that each
aggregate their own tab-separated fields, the hand-off dirsql pays as one
JSON object per line. No mocks: real console script, real process,
real filesystem, real parser spawn.
"""

from __future__ import annotations

import shutil

import pytest

from .speed_of_light import (
    AGG_AWK,
    MERGE_AWK,
    Native,
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

COLUMNS = ("status", "n", "total_words")
QUERY = (
    "SELECT status, count(*) AS n, sum(words) AS total_words FROM notes"
    " GROUP BY status ORDER BY status"
)

CONFIG = """\
[[table]]
name    = "notes"
glob    = "notes/**/*.md"
ddl     = "CREATE TABLE notes (status TEXT, words INTEGER)"
on-file = "awk -f notes.awk"
"""

PARSE_FIELDS = r"""
FNR == 1 { fence = 0; status = ""; words = 0 }
/^---$/ { fence++; if (fence == 2) { emit() } ; next }
fence == 1 && /^status: / { status = substr($0, 9) }
fence == 1 && /^words: / { words = substr($0, 8) + 0 }
"""

NOTES_AWK = (
    PARSE_FIELDS
    + r"""
function emit() { print "{\"status\":\"" status "\",\"words\":" words "}" }
"""
)

FIELDS_AWK = (
    PARSE_FIELDS
    + r"""
function emit() { print status "\t" words }
"""
)

BATCHED = (
    '-0 -P"$(nproc)" -n 1000 sh -c'
    ' \'awk -f fields.awk "$@" | awk -F "\\t" -f agg.awk\' _'
    " | awk -F '\\t' -f merge.awk | sort"
)
NATIVE = [
    Native(
        "find+awk",
        ("find",),
        "{bin} notes -name '*.md' -exec awk -f fields.awk {} +"
        " | awk -F '\\t' -f agg.awk | sort",
    ),
    Native(
        "find+xargs-P",
        ("find",),
        "{bin} notes -name '*.md' -print0 | xargs " + BATCHED,
    ),
    Native(
        "fd+xargs-P",
        ("fd", "fdfind"),
        "{bin} --no-ignore -0 -e md . notes | xargs " + BATCHED,
    ),
]

STATUSES = ("draft", "review", "published", "archived")
BODY = "\n".join(["Some body text that follows the frontmatter block."] * 20)


def build_notes(root, lo, hi):
    """Notes `lo` through `hi - 1` across a hundred folders, each with a
    five-key frontmatter block and a body that repeats a `---` rule below it."""
    if lo == 0:
        root.mkdir(parents=True)
        (root / ".dirsql.toml").write_text(CONFIG)
        (root / "notes.awk").write_text(NOTES_AWK)
        (root / "fields.awk").write_text(FIELDS_AWK)
        (root / "agg.awk").write_text(AGG_AWK)
        (root / "merge.awk").write_text(MERGE_AWK)
    for i in range(lo, hi):
        path = root / "notes" / f"dir-{i % 100}" / f"note-{i}.md"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            "---\n"
            f"title: Note {i}\n"
            f"status: {STATUSES[(i * 7) % len(STATUSES)]}\n"
            f"words: {i % 500}\n"
            "tags: alpha, beta\n"
            "author: someone\n"
            "---\n"
            f"{BODY}\n"
        )


def native_rows(proc):
    rows = []
    for line in proc.stdout.splitlines():
        status, n, total = line.split("\t")
        rows.append((status, int(n), int(total)))
    return rows


def describe_frontmatter_parse_speed_of_light():
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
            lambda lo, hi: build_notes(root, lo, hi),
            baseline(natives),
            start=2048,
            ceiling=2**18,
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
        assert_speed_of_light("frontmatter", result, startup)
