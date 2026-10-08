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
    cli,
    dirsql_rows,
    grow_until_native_takes_a_second,
    paired,
    startup_seconds,
    timed,
    timed_native,
)

MARKDOWN = (
    "find . -mindepth 1 -name '.*' -prune -o -type f -name '*.md' -printf '%P\\n'"
)
DOTFILES = "find . -mindepth 1 -name '.*' -prune -type f -printf '%P\\n'"
UNDER_CACHE = "find .cache -mindepth 1 -name '.*' -prune -o -type f -name '*.md' -print"
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
    proc, seconds = timed_native(["bash", "-c", script], root)
    assert proc.returncode == 0, proc.stderr
    return sorted(proc.stdout.splitlines()), seconds


def describe_dotfile_glob_speed_of_light():
    @pytest.fixture(scope="module")
    def tree(tmp_path_factory):
        root = tmp_path_factory.mktemp("dotfiles") / "tree"
        root.mkdir()
        try:
            n = grow_until_native_takes_a_second(
                lambda lo, hi: build_tree(root, lo, hi),
                lambda: native(root, MARKDOWN),
                start=100_000,
                ceiling=2**22,
            )
            yield root, n, startup_seconds()
        finally:
            shutil.rmtree(root, ignore_errors=True)

    @pytest.mark.parametrize(
        ("glob", "script"),
        [
            ("./**/*.md", MARKDOWN),
            ("./**/.*", DOTFILES),
            ("./.cache/**/*.md", UNDER_CACHE),
        ],
    )
    def it_matches_native_files_within_the_bar(tree, glob, script):
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
        assert len(expected) == matched(n, glob)

        assert actual == expected
        assert_speed_of_light(glob, result, startup)
