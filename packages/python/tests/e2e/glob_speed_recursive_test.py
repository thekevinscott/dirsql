"""Glob speed: recursive `'./**/*.md'`, the literal prefix `'./a/b/**/*.md'`,
and one equality lookup on `path` over that prefix.

A deep, wide tree of markdown files mixed with other files, dotfiles and
dot-named directories that globstar with dotglob off must not return. A
quarter of it sits under `a/b`, the rest beside it, so a prefixed glob that
walked the whole tree would read four times what it needs. Natives are `find`
pruning dot-named entries, `fd` and `rg --files`. No mocks: real
console script, real process, real filesystem.
"""

from __future__ import annotations

import shutil

import pytest

from .speed_of_light import (
    FASTEST,
    Native,
    agreed_rows,
    assert_speed_of_light,
    baseline,
    cli,
    dirsql_rows,
    fd,
    globstar,
    grow_until_native_takes_a_second,
    paired,
    rg,
    shell_natives,
    startup_seconds,
    suite_only,
    timed,
)

ALL_NATIVE = [
    Native(
        "find",
        ("find",),
        "{bin} . -mindepth 1 -name '.*' -prune -o -type f -name '*.md' -printf '%P\\n'",
    ),
    fd("--glob '*.md'"),
    rg("-g '*.md' -g '!.*'"),
    *suite_only(globstar("./**/*.md")),
]
PREFIX_NATIVE = [
    Native(
        "find",
        ("find",),
        "{bin} a/b -mindepth 1 -name '.*' -prune -o -type f -name '*.md' -print",
    ),
    fd("--glob '*.md' a/b"),
    rg("-g '*.md' -g '!.*' a/b"),
    *suite_only(globstar("./a/b/**/*.md")),
]
LOOKUP = "a/b/x1/y0/z0/n4.md"
LOOKUP_NATIVE = [
    Native(
        "find",
        ("find",),
        f"{{bin}} a/b -mindepth 1 -name '.*' -prune -o -type f -path '{LOOKUP}' -print",
    ),
    fd(f"--full-path --glob '**/{LOOKUP}' a/b"),
]
CASES = [
    ("./**/*.md", "./**/*.md", ALL_NATIVE, "", FASTEST),
    ("./a/b/**/*.md", "./a/b/**/*.md", PREFIX_NATIVE, "a/b", ""),
]
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


def describe_recursive_glob_speed_of_light():
    @pytest.fixture(scope="module")
    def tree(tmp_path_factory):
        root = tmp_path_factory.mktemp("recursive") / "tree"
        root.mkdir()
        try:
            n = grow_until_native_takes_a_second(
                lambda lo, hi: build_tree(root, lo, hi),
                baseline(shell_natives(root, ALL_NATIVE)),
                start=100_000,
                ceiling=2**22,
            )
            yield root, n, startup_seconds()
        finally:
            shutil.rmtree(root, ignore_errors=True)

    @pytest.mark.parametrize(("name", "glob", "specs", "prefix", "bar"), CASES)
    def it_matches_native_files_within_the_bar(tree, name, glob, specs, prefix, bar):
        root, n, startup = tree

        def dirsql(timeout):
            proc, seconds = timed(
                [cli(), "query", f"SELECT path FROM '{glob}'"],
                root,
                timeout=timeout,
            )
            return sorted(row for (row,) in dirsql_rows(proc, ("path",))), seconds

        result = paired(shell_natives(root, specs, bar=bar), dirsql, startup)
        expected, actual = agreed_rows(result), result.dirsql_rows
        assert len(expected) == matched(n, prefix)

        assert actual == expected
        assert_speed_of_light(name, result, startup)

    def it_finds_one_path_by_equality_within_the_bar(tree):
        root, _, startup = tree
        name = "./a/b/**/*.md WHERE path ="

        def dirsql(timeout):
            proc, seconds = timed(
                [
                    cli(),
                    "query",
                    f"SELECT path FROM './a/b/**/*.md' WHERE path = '{LOOKUP}'",
                ],
                root,
                timeout=timeout,
            )
            return sorted(row for (row,) in dirsql_rows(proc, ("path",))), seconds

        result = paired(shell_natives(root, LOOKUP_NATIVE), dirsql, startup)

        assert agreed_rows(result) == [LOOKUP]
        assert result.dirsql_rows == [LOOKUP]
        assert_speed_of_light(name, result, startup)
