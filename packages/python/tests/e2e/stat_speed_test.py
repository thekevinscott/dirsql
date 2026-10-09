"""Stat-column speed: `SELECT path, size, mtime` over a large tree.

A deep, wide tree of files with varied sizes and modification times. Natives
are a single `find -printf` that stats every file once and prints the same
three fields, and `fd` handing its paths to parallel `stat` processes. No mocks: real console script, real process, real filesystem.
"""

from __future__ import annotations

import os
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

NATIVE = [
    Native("find", ("find",), "{bin} . -type f -printf '%P\\t%s\\t%T@\\n'"),
    Native(
        "fd+xargs-stat",
        ("fd", "fdfind"),
        '{bin} --no-ignore --strip-cwd-prefix --type f -0 | xargs -0 -P"$(nproc)" -n 100'
        " stat --printf '%n\\t%s\\t%Y\\n'",
    ),
]
QUERY = "SELECT path, size, mtime FROM './**/*'"
FANOUT = 8
EPOCH = 1_700_000_000


def build_tree(root, lo, hi):
    """Files `lo` through `hi - 1`, each three directories deep, holding
    `i % 97` bytes and stamped with a distinct modification time."""
    for i in range(lo, hi):
        folder = (
            root
            / f"x{i % FANOUT}"
            / f"y{i // FANOUT % FANOUT}"
            / f"z{i // FANOUT**2 % FANOUT}"
        )
        folder.mkdir(parents=True, exist_ok=True)
        path = folder / f"n{i}.dat"
        path.write_bytes(b"x" * (i % 97))
        os.utime(path, (EPOCH + i, EPOCH + i))


def parse(proc):
    rows = []
    for line in proc.stdout.splitlines():
        path, size, mtime = line.split("\t")
        rows.append((path, int(size), int(mtime.split(".")[0])))
    return sorted(rows)


def describe_stat_column_speed_of_light():
    @pytest.fixture
    def root(tmp_path):
        tree = tmp_path / "stat"
        tree.mkdir()
        try:
            yield tree
        finally:
            shutil.rmtree(tree, ignore_errors=True)

    def it_matches_native_stat_rows_within_the_bar(root):
        startup = startup_seconds()
        natives = shell_natives(root, NATIVE, parse)
        n = grow_until_native_takes_a_second(
            lambda lo, hi: build_tree(root, lo, hi),
            baseline(natives),
            start=100_000,
            ceiling=2**22,
        )

        def dirsql(timeout):
            proc, seconds = timed(
                [cli(), "query", QUERY],
                root,
                timeout=timeout,
            )
            return sorted(dirsql_rows(proc, ("path", "size", "mtime"))), seconds

        result = paired(natives, dirsql, startup)
        expected, actual = agreed_rows(result), result.dirsql_rows
        assert len(expected) == n

        assert actual == expected
        assert_speed_of_light(QUERY, result, startup)
