"""Parse speed: many markdown files with frontmatter.

A tree of notes, each opening with a `---` frontmatter block, parsed by one
awk process named in `.dirsql.toml`; the question is how many notes carry each
status and their summed word counts. Native runs the same awk over every file
and aggregates its tab-separated fields with a second awk, the hand-off
dirsql pays as one JSON object per line. No mocks: real console script, real process,
real filesystem, real parser spawn.
"""

from __future__ import annotations

import shutil

import pytest

from .speed_of_light import (
    assert_speed_of_light,
    best_of,
    cli,
    dirsql_rows,
    dirsql_timeout,
    grow_until_native_takes_a_second,
    hopeless_seconds,
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

NATIVE = (
    "find notes -name '*.md' -exec awk -f fields.awk {} +"
    " | awk -F '\\t' '{ n[$1]++; s[$1] += $2 }"
    " END { for (k in n) print k \"\\t\" n[k] \"\\t\" s[k] }'"
    " | sort"
)

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
    assert proc.returncode == 0, proc.stderr
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

        def native():
            proc, seconds = timed(["sh", "-c", NATIVE], root, timeout=600)
            return native_rows(proc), seconds

        grow_until_native_takes_a_second(
            lambda lo, hi: build_notes(root, lo, hi),
            native,
            start=2048,
            ceiling=2**18,
        )
        expected, native_seconds = best_of(native, "native")
        assert len(expected) == len(STATUSES), expected

        def dirsql():
            proc, seconds = timed(
                [cli(), "query", QUERY, "-c", ".dirsql.toml"],
                root,
                timeout=dirsql_timeout(native_seconds),
            )
            return dirsql_rows(proc, COLUMNS), seconds

        actual, dirsql_seconds = best_of(
            dirsql, "dirsql", hopeless_seconds(native_seconds, startup)
        )

        assert actual == expected
        assert_speed_of_light("frontmatter", native_seconds, dirsql_seconds, startup)
