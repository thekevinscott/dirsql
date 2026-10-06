"""Glob speed: `'./*/*.md'` stops where the pattern cannot match deeper.

Few directories at the top, each holding markdown files mixed with other
files, dotfiles, and a deep chain of directories as large again as the
files the glob can match. A walk that descended past depth two would read
the chain for nothing. Native is `find` bounded at depth two. No mocks:
real console script, real process, real filesystem.
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

GLOB = "./*/*.md"
NATIVE = (
    "find . -mindepth 2 -maxdepth 2 -type f -name '*.md' ! -path '*/.*' -printf '%P\\n'"
)
TOPS = 32
FANOUT = 8


def top(i):
    """Item `i`'s top-level directory; every seventeenth is dot-named."""
    name = f"t{i % TOPS}"
    return f".{name}" if i % TOPS == 17 else name


def build_tree(root, lo, hi):
    """Items `lo` through `hi - 1`, each a file directly under its top-level
    directory and another five directories below it. Every ninth file is a
    `.txt` and every fiftieth a dotfile."""
    for i in range(lo, hi):
        shallow = root / top(i)
        j = i // TOPS
        deep = (
            shallow
            / f"d{j % FANOUT}"
            / f"e{j // FANOUT % FANOUT}"
            / f"f{j // FANOUT**2 % FANOUT}"
            / "g"
            / "h"
        )
        deep.mkdir(parents=True, exist_ok=True)
        if i % 9 == 0:
            name = f"n{i}.txt"
        elif i % 50 == 0:
            name = f".h{i}.md"
        else:
            name = f"n{i}.md"
        (shallow / name).touch()
        (deep / name).touch()


def matched(n):
    return sum(1 for i in range(n) if i % TOPS != 17 and i % 9 and i % 50)


def native(root):
    proc, seconds = timed(["bash", "-c", NATIVE], root, timeout=600)
    assert proc.returncode == 0, proc.stderr
    return sorted(proc.stdout.splitlines()), seconds


def describe_depth_bound_glob_speed_of_light():
    @pytest.fixture
    def root(tmp_path):
        tree = tmp_path / "depth-bound"
        tree.mkdir()
        try:
            yield tree
        finally:
            shutil.rmtree(tree, ignore_errors=True)

    def it_matches_native_files_within_the_bar(root):
        startup = startup_seconds()
        n = grow_until_native_takes_a_second(
            lambda lo, hi: build_tree(root, lo, hi),
            lambda: native(root),
            start=100_000,
            ceiling=2**22,
        )
        expected, native_seconds = best_of(lambda: native(root), "native")
        assert len(expected) == matched(n)

        def dirsql():
            proc, seconds = timed(
                [cli(), "query", f"SELECT path FROM '{GLOB}'"],
                root,
                timeout=dirsql_timeout(native_seconds),
            )
            return sorted(row for (row,) in dirsql_rows(proc, ("path",))), seconds

        actual, dirsql_seconds = best_of(
            dirsql, "dirsql", hopeless_seconds(native_seconds, startup)
        )

        assert actual == expected
        assert_speed_of_light(GLOB, native_seconds, dirsql_seconds, startup)
