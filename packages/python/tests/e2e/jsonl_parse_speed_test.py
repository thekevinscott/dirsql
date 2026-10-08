"""Parse speed: a few huge JSONL files.

Four large event logs, one JSON object per line, each projected to the two
fields the question needs by `jq -c '{level, ms}'` named in `.dirsql.toml`; the
question is the event count and summed duration per level. Native streams the
same two fields out of every line with one `jq` process and aggregates them in
awk. No mocks: real console script, real process, real filesystem, real
parser spawn.
"""

from __future__ import annotations

import json
import shutil

import pytest

from .speed_of_light import (
    assert_speed_of_light,
    cli,
    dirsql_rows,
    grow_until_native_takes_a_second,
    paired,
    startup_seconds,
    timed,
    timed_native,
)

COLUMNS = ("level", "n", "total_ms")
QUERY = (
    "SELECT level, count(*) AS n, sum(ms) AS total_ms FROM events"
    " GROUP BY level ORDER BY level"
)

CONFIG = """\
[[table]]
name    = "events"
glob    = "logs/*.jsonl"
ddl     = "CREATE TABLE events (level TEXT, ms INTEGER)"
on-file = "jq -c '{level, ms}'"
"""

NATIVE = (
    "jq -r '[.level, .ms] | @tsv' logs/*.jsonl"
    " | awk -F '\\t' '{ n[$1]++; s[$1] += $2 }"
    ' END { for (k in n) print k "\\t" n[k] "\\t" s[k] }\''
    " | sort"
)

LEVELS = ("debug", "info", "warn", "error")
FILES = 4
PADDING = " ".join(["payload"] * 20)


def build_logs(root, lo, hi):
    """Events `lo` through `hi - 1`, appended round-robin to four files."""
    if lo == 0:
        (root / "logs").mkdir(parents=True)
        (root / ".dirsql.toml").write_text(CONFIG)
    handles = [
        open(root / "logs" / f"events-{f}.jsonl", "a", encoding="utf-8")
        for f in range(FILES)
    ]
    try:
        for i in range(lo, hi):
            event = {
                "id": i,
                "level": LEVELS[(i * 7) % len(LEVELS)],
                "ms": i % 997,
                "msg": f"event {i} {PADDING}",
            }
            handles[i % FILES].write(json.dumps(event) + "\n")
    finally:
        for handle in handles:
            handle.close()


def native_rows(proc):
    assert proc.returncode == 0, proc.stderr
    rows = []
    for line in proc.stdout.splitlines():
        level, n, total = line.split("\t")
        rows.append((level, int(n), int(total)))
    return rows


def describe_jsonl_parse_speed_of_light():
    @pytest.fixture
    def root(tmp_path):
        tree = tmp_path / "events"
        tree.mkdir()
        try:
            yield tree
        finally:
            shutil.rmtree(tree, ignore_errors=True)

    def it_matches_native_rows_within_the_bar(root):
        startup = startup_seconds()

        def native():
            proc, seconds = timed_native(["sh", "-c", NATIVE], root)
            return native_rows(proc), seconds

        grow_until_native_takes_a_second(
            lambda lo, hi: build_logs(root, lo, hi),
            native,
            start=2**17,
            ceiling=2**24,
        )

        def dirsql(timeout):
            proc, seconds = timed(
                [cli(), "query", QUERY, "-c", ".dirsql.toml"],
                root,
                timeout=timeout,
            )
            return dirsql_rows(proc, COLUMNS), seconds

        result = paired(native, dirsql, startup)
        expected, actual = result.native_rows, result.dirsql_rows
        assert len(expected) == len(LEVELS), expected

        assert actual == expected
        assert_speed_of_light("jsonl", result, startup)
