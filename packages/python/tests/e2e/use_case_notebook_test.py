"""Use case 3, the lab notebook: one file, many rows.

One markdown file of dated `## YYYY-MM-DD` sections, split into rows by a
parser script. Native is the parser alone piped into a sort-and-head. dirsql
runs the same parser once through `--on-file` and sorts in SQLite. No mocks:
real console script, real process, real filesystem, real parser spawn.
"""

from __future__ import annotations

import datetime
import shutil

import pytest

from .speed_of_light import (
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

COLUMNS = ("date", "words")
QUERY = "SELECT date, words FROM './notebook.md' ORDER BY date DESC LIMIT 5"

EXTRACT = """\
import json, re, sys

for path in sys.argv[1:]:
    text = open(path, encoding="utf-8").read()
    for date, body in re.findall(r"^## (\\d{4}-\\d\\d-\\d\\d)\\n(.*?)(?=^## |\\Z)", text, re.S | re.M):
        print(json.dumps({"date": date, "body": body.strip(), "words": len(body.split())}))
"""

TOP_FIVE = (
    "import json, sys;"
    " rows = [json.loads(l) for l in sys.stdin];"
    " rows.sort(key=lambda r: r['date'], reverse=True);"
    " [print(r['date'], r['words'], sep='\\t') for r in rows[:5]]"
)

NATIVE = [
    Native(
        "python",
        ("python3",),
        f'{{bin}} extract.py notebook.md | {{bin}} -c "{TOP_FIVE}"',
    )
]

WORDS = [
    "ran",
    "the",
    "cells",
    "again",
    "and",
    "logged",
    "every",
    "reading",
    "before",
    "lunch",
]


def build_notebook(root, n):
    """`n` sections, newest first, one per day back from a fixed date, each
    with a distinct word count so the rows carry something to compare."""
    root.mkdir(parents=True, exist_ok=True)
    (root / "extract.py").write_text(EXTRACT)
    newest = datetime.date(2026, 9, 23)
    chunks = ["# Lab notebook\n\nOne file. Newest entry at the top.\n"]
    for i in range(n):
        day = newest - datetime.timedelta(days=i)
        body = " ".join(WORDS[j % len(WORDS)] for j in range(3 + i % 37))
        chunks.append(f"\n## {day.isoformat()}\n\n{body}\n")
    (root / "notebook.md").write_text("".join(chunks))


def native_rows(proc):
    rows = []
    for line in proc.stdout.splitlines():
        date, words = line.split("\t")
        rows.append((date, int(words)))
    return rows


def describe_notebook_speed_of_light():
    @pytest.fixture
    def root(tmp_path):
        tree = tmp_path / "notebook"
        try:
            yield tree
        finally:
            shutil.rmtree(tree, ignore_errors=True)

    def it_matches_native_rows_within_the_bar(root):
        startup = startup_seconds()

        natives = shell_natives(root, NATIVE, native_rows, shell="sh")

        grow_until_native_takes_a_second(
            lambda _, n: build_notebook(root, n),
            baseline(natives),
            start=20_000,
            ceiling=2**22,
        )

        def dirsql(timeout):
            proc, seconds = timed(
                [cli(), "query", QUERY, "--on-file", "python3 extract.py"],
                root,
                timeout=timeout,
            )
            return dirsql_rows(proc, COLUMNS), seconds

        result = paired(natives, dirsql, startup)
        expected, actual = agreed_rows(result), result.dirsql_rows
        assert len(expected) == 5, expected

        assert actual == expected
        assert_speed_of_light("notebook", result, startup)
