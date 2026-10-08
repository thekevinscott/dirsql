"""Glob speed: a symlinked directory and a trailing `/`.

A tree whose `src/vendor` is a symlink to a large directory outside it.
`'./**/*.md'` follows the link the way bash globstar does: `**` stops on it
and `*.md` lists the files directly inside, never the directories below.
`'./docs/'` lists the files directly in `docs`, however deep `docs` goes.
Native for the first is bash globstar alone, the reference for which links it
takes: `find`, `fd` and `rg` either skip the link or descend through it.
Natives for the second are `find` bounded at one level, `fd`, `rg --files`
and a bash glob. No mocks: real console
script, real process, real filesystem.
"""

from __future__ import annotations

import shutil

import pytest

from .speed_of_light import (
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
    timed,
)

LINKED = [globstar("./**/*.md")]
DOCS = [
    Native(
        "find",
        ("find",),
        "{bin} docs -mindepth 1 -maxdepth 1 -type f ! -name '.*' -printf 'docs/%P\\n'",
    ),
    fd("--max-depth 1 . docs"),
    rg("--max-depth 1 docs"),
    Native(
        "bash-glob",
        ("bash",),
        '{bin} -c \'for f in docs/*; do if [ -f "$f" ]; then printf "%s\\n" "$f"; fi; done\'',
    ),
]
FANOUT = 8


def item(i):
    """Item `i`'s area and path below it. Areas: 0 directly in `docs`, 1 deep
    in `docs`, 2 directly in the linked directory, 3 deep in it, 4 deep in
    `src`. Every ninth is a `.txt`."""
    j = i // 5
    nest = f"x{j % FANOUT}/y{j // FANOUT % FANOUT}/z{j // FANOUT**2 % FANOUT}"
    name = f"f{i}.txt" if i % 9 == 0 else f"f{i}.md"
    area = i % 5
    folder = ("docs", f"docs/{nest}", "src/vendor", f"src/vendor/{nest}", f"src/{nest}")
    return area, f"{folder[area]}/{name}"


def build_tree(root, lo, hi):
    """Items `lo` through `hi - 1`; `src/vendor` is a link to `vendor`
    beside the root, so its files are reached through the link alone."""
    outside = root.parent / "vendor"
    if lo == 0:
        outside.mkdir()
        (root / "src").mkdir()
        (root / "src" / "vendor").symlink_to(outside, target_is_directory=True)
    for i in range(lo, hi):
        area, rel = item(i)
        if area in (2, 3):
            rel = rel.removeprefix("src/vendor/")
            path = outside / rel
        else:
            path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.touch()


def matched(n, glob):
    def hit(i):
        area, rel = item(i)
        if glob == "./docs/":
            return area == 0
        return area != 3 and rel.endswith(".md")

    return sum(1 for i in range(n) if hit(i))


def describe_symlink_and_trailing_slash_glob_speed_of_light():
    @pytest.fixture(scope="module")
    def tree(tmp_path_factory):
        root = tmp_path_factory.mktemp("links") / "tree"
        root.mkdir()
        try:
            n = grow_until_native_takes_a_second(
                lambda lo, hi: build_tree(root, lo, hi),
                baseline(shell_natives(root, LINKED)),
                start=100_000,
                ceiling=2**22,
            )
            yield root, n, startup_seconds()
        finally:
            shutil.rmtree(root.parent, ignore_errors=True)

    @pytest.mark.parametrize(
        ("glob", "specs"),
        [("./**/*.md", LINKED), ("./docs/", DOCS)],
    )
    def it_matches_native_files_within_the_bar(tree, glob, specs):
        root, n, startup = tree

        def dirsql(timeout):
            proc, seconds = timed(
                [cli(), "query", f"SELECT path FROM '{glob}'"],
                root,
                timeout=timeout,
            )
            return sorted(row for (row,) in dirsql_rows(proc, ("path",))), seconds

        result = paired(shell_natives(root, specs), dirsql, startup)
        expected, actual = agreed_rows(result), result.dirsql_rows
        assert len(expected) == matched(n, glob)

        assert actual == expected
        assert_speed_of_light(glob, result, startup)
