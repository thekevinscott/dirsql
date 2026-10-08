"""Glob speed: recursive `'./**/*.md'` and the literal prefix `'./a/b/**/*.md'`.

A deep, wide tree of markdown files mixed with other files, dotfiles and
dot-named directories that globstar with dotglob off must not return. A
quarter of it sits under `a/b`, the rest beside it, so a prefixed glob that
walked the whole tree would read four times what it needs. Native is `find`
pruning dot-named entries, faster here than bash globstar. No mocks: real
console script, real process, real filesystem.
"""

from __future__ import annotations

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

ALL_NATIVE = (
    "find . -mindepth 1 -name '.*' -prune -o -type f -name '*.md' -printf '%P\\n'"
)
PREFIX_NATIVE = "find a/b -mindepth 1 -name '.*' -prune -o -type f -name '*.md' -print"
FANOUT = 8


def home(i):
    """The top of item `i`'s subtree: a quarter under `a/b`, the rest beside it."""
    return ("a/b", "a/c", "t1", "t2")[i % 4]


def build_tree(root, lo, hi):
    """Items `lo` through `hi - 1`, each a file three directories below its
    home. Every ninth is a `.txt`, every fiftieth a dotfile, and every
    twenty-fifth sits in a dot-named directory."""
    for i in range(lo, hi):
        j = i // 4
        folder = (
            root
            / home(i)
            / f"x{j % FANOUT}"
            / f"y{j // FANOUT % FANOUT}"
            / f"z{j // FANOUT**2 % FANOUT}"
        )
        if i % 25 == 0:
            folder = folder / ".cache"
        folder.mkdir(parents=True, exist_ok=True)
        if i % 9 == 0:
            name = f"n{i}.txt"
        elif i % 50 == 0:
            name = f".h{i}.md"
        else:
            name = f"n{i}.md"
        (folder / name).touch()


def matched(n, prefix):
    return sum(
        1
        for i in range(n)
        if home(i).startswith(prefix) and i % 25 and i % 9 and i % 50
    )


def native(root, script):
    proc, seconds = timed_native(["bash", "-c", script], root)
    assert proc.returncode == 0, proc.stderr
    return sorted(proc.stdout.splitlines()), seconds


def describe_recursive_glob_speed_of_light():
    @pytest.fixture(scope="module")
    def tree(tmp_path_factory):
        root = tmp_path_factory.mktemp("recursive") / "tree"
        root.mkdir()
        try:
            n = grow_until_native_takes_a_second(
                lambda lo, hi: build_tree(root, lo, hi),
                lambda: native(root, ALL_NATIVE),
                start=100_000,
                ceiling=2**22,
            )
            yield root, n, startup_seconds()
        finally:
            shutil.rmtree(root, ignore_errors=True)

    @pytest.mark.parametrize(
        ("glob", "script", "prefix"),
        [("./**/*.md", ALL_NATIVE, ""), ("./a/b/**/*.md", PREFIX_NATIVE, "a/b")],
    )
    def it_matches_native_files_within_the_bar(tree, glob, script, prefix):
        root, n, startup = tree

        def dirsql(timeout):
            proc, seconds = timed(
                [cli(), "query", f"SELECT path FROM '{glob}'"],
                root,
                timeout=timeout,
            )
            return sorted(row for (row,) in dirsql_rows(proc, ("path",))), seconds

        result = paired(lambda: native(root, script), dirsql, startup)
        expected, actual = result.native_rows, result.dirsql_rows
        assert len(expected) == matched(n, prefix)

        assert actual == expected
        assert_speed_of_light(glob, result, startup)
