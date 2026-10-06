"""Glob speed: dotfiles stay hidden unless the pattern spells the dot.

A deep, wide tree where half the top-level directories are dot-named and
dot-named directories and dotfiles recur below. `'./**/*.md'` must skip
every one of them; `'./**/.*'` reaches dotfiles because the pattern spells
the dot, and `'./.cache/**/*.md'` reaches under a dot-named directory
because the pattern names it. Native is `find` pruning what bash globstar
with dotglob off would not enter. No mocks: real console script, real
process, real filesystem.
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

MARKDOWN = (
    "find . -mindepth 1 -name '.*' -prune -o -type f -name '*.md' -printf '%P\\n'"
)
DOTFILES = "find . -mindepth 1 -name '.*' -prune -type f -printf '%P\\n'"
UNDER_CACHE = (
    "find .cache -mindepth 1 -name '.*' -prune -o -type f -name '*.md' -print"
)
FANOUT = 8


def home(i):
    """The top of item `i`'s subtree: half of them dot-named."""
    return ("a", "b", ".cache", ".tmp")[i % 4]


def build_tree(root, lo, hi):
    """Items `lo` through `hi - 1`, each a file three directories below its
    home. Every seventh sits in a dot-named directory, every fifth is a
    dotfile, and every ninth a `.txt`."""
    for i in range(lo, hi):
        j = i // 4
        folder = (
            root
            / home(i)
            / f"x{j % FANOUT}"
            / f"y{j // FANOUT % FANOUT}"
            / f"z{j // FANOUT**2 % FANOUT}"
        )
        if i % 7 == 0:
            folder = folder / ".d"
        folder.mkdir(parents=True, exist_ok=True)
        if i % 5 == 0:
            name = f".h{i}.md"
        elif i % 9 == 0:
            name = f"n{i}.txt"
        else:
            name = f"n{i}.md"
        (folder / name).touch()


def matched(n, glob):
    def hit(i):
        visible = i % 7 != 0
        markdown = i % 5 != 0 and i % 9 != 0
        if glob == "./**/*.md":
            return not home(i).startswith(".") and visible and markdown
        if glob == "./**/.*":
            return not home(i).startswith(".") and visible and i % 5 == 0
        return home(i) == ".cache" and visible and markdown

    return sum(1 for i in range(n) if hit(i))


def native(root, script):
    proc, seconds = timed(["bash", "-c", script], root, timeout=600)
    assert proc.returncode == 0, proc.stderr
    return sorted(proc.stdout.splitlines()), seconds


def describe_dotfile_glob_speed_of_light():
    @pytest.fixture
    def root(tmp_path):
        tree = tmp_path / "dotfiles"
        tree.mkdir()
        try:
            yield tree
        finally:
            shutil.rmtree(tree, ignore_errors=True)

    @pytest.mark.parametrize(
        ("glob", "script"),
        [
            ("./**/*.md", MARKDOWN),
            ("./**/.*", DOTFILES),
            ("./.cache/**/*.md", UNDER_CACHE),
        ],
    )
    def it_matches_native_files_within_the_bar(root, glob, script):
        startup = startup_seconds()
        n = grow_until_native_takes_a_second(
            lambda lo, hi: build_tree(root, lo, hi),
            lambda: native(root, script),
            start=100_000,
            ceiling=2**22,
        )
        expected, native_seconds = best_of(lambda: native(root, script), "native")
        assert len(expected) == matched(n, glob)

        def dirsql():
            proc, seconds = timed(
                [cli(), "query", f"SELECT path FROM '{glob}'"],
                root,
                timeout=dirsql_timeout(native_seconds),
            )
            return sorted(row for (row,) in dirsql_rows(proc, ("path",))), seconds

        actual, dirsql_seconds = best_of(
            dirsql, "dirsql", hopeless_seconds(native_seconds, startup)
        )

        assert actual == expected
        assert_speed_of_light(glob, native_seconds, dirsql_seconds, startup)
