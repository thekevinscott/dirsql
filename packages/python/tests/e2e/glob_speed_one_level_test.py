"""Glob speed: one-level listings, `'./*'`.

A wide flat root and a wide flat `docs/`, each beside subdirectories full of
files that a one-level listing must not read, plus dotfiles it must hide.
Native is `find -mindepth 1 -maxdepth 1 -type f ! -name '.*'`, faster here
than a bash glob, which has to stat every entry to drop the directories.
No mocks: real console script, real process, real filesystem.
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

ROOT_NATIVE = "find . -mindepth 1 -maxdepth 1 -type f ! -name '.*' -printf '%P\\n'"
FILES_PER_SUBDIR = 1000


def build_tree(root, lo, hi):
    """Items `lo` through `hi - 1`: one file at the root, one in `docs/` and
    one in a subdirectory of each. Every fiftieth root and `docs/` file is a
    dotfile."""
    for i in range(lo, hi):
        sub = f"sub{i // FILES_PER_SUBDIR}"
        for top in (root, root / "docs"):
            (top / sub).mkdir(parents=True, exist_ok=True)
            (top / sub / f"s{i}.md").touch()
            name = f".h{i}.md" if i % 50 == 0 else f"f{i}.md"
            (top / name).touch()


def native(root, script):
    proc, seconds = timed_native(["bash", "-c", script], root)
    assert proc.returncode == 0, proc.stderr
    return sorted(proc.stdout.splitlines()), seconds


def describe_one_level_glob_speed_of_light():
    @pytest.fixture(scope="module")
    def tree(tmp_path_factory):
        root = tmp_path_factory.mktemp("one-level")
        try:
            n = grow_until_native_takes_a_second(
                lambda lo, hi: build_tree(root, lo, hi),
                lambda: native(root, ROOT_NATIVE),
                start=100_000,
                ceiling=2**21,
            )
            yield root, n, startup_seconds()
        finally:
            shutil.rmtree(root, ignore_errors=True)

    @pytest.mark.parametrize(
        ("glob", "script"),
        [("./*", ROOT_NATIVE)],
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
        assert len(expected) == n - len(range(0, n, 50))

        assert actual == expected
        assert_speed_of_light(glob, result, startup)
