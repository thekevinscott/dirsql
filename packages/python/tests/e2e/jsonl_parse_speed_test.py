"""Parse speed: a few huge JSONL files.

Four large event logs, one JSON object per line, each projected to the two
fields the question needs by `jq -c '{level, ms}'` named in `.dirsql.toml`; the
question is the event count and summed duration per level. Natives stream the
same two fields out of every line with one `jq` process, or with one per file
under `xargs -P`, and aggregate them in awk. No mocks: real console script, real process, real filesystem, real
parser spawn.
"""

from __future__ import annotations

import json
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

NATIVE = [
    Native(
        "jq",
        ("jq",),
        "{bin} -r '[.level, .ms] | @tsv' logs/*.jsonl | awk -F '\\t' -f agg.awk | sort",
    ),
    Native(
        "jq+xargs-P",
        ("jq",),
        "printf '%s\\0' logs/*.jsonl | xargs -0 -P\"$(nproc)\" -n 1 sh -c"
        ' \'{bin} -r "[.level, .ms] | @tsv" "$@" | awk -F "\\t" -f agg.awk\' _'
        " | awk -F '\\t' -f merge.awk | sort",
    ),
]

LEVELS = ("debug", "info", "warn", "error")
FILES = 4
PADDING = " ".join(["payload"] * 20)


def build_logs(root, lo, hi):
    """Events `lo` through `hi - 1`, appended round-robin to four files."""
    if lo == 0:
        (root / "logs").mkdir(parents=True)
        (root / ".dirsql.toml").write_text(CONFIG)
        (root / "agg.awk").write_text(AGG_AWK)
        (root / "merge.awk").write_text(MERGE_AWK)
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

        natives = shell_natives(root, NATIVE, native_rows, shell="sh")
        grow_until_native_takes_a_second(
            lambda lo, hi: build_logs(root, lo, hi),
            baseline(natives),
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

        result = paired(natives, dirsql, startup)
        expected, actual = agreed_rows(result), result.dirsql_rows
        assert len(expected) == len(LEVELS), expected

        assert actual == expected
        assert_speed_of_light("jsonl", result, startup)
