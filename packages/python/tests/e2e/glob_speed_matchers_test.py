"""Glob speed: braces, `?`, `[...]` and POSIX classes under `**`.

One deep, wide tree of markdown files whose names start with different
letters, a digit or `file` plus one or two characters, so each matcher keeps
a different share of them. Natives are `find` with the same name test, `fd`,
`rg --files` and bash globstar, which all return what globstar does with
dotglob off. No mocks: real console
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

FIND = "{{bin}} . -mindepth 1 -name '.*' -prune -o -type f {test} -printf '%P\\n'"
CASES = [
    ("./**/{a,b}*.md", "\\( -name 'a*.md' -o -name 'b*.md' \\)", "{a,b}*.md"),
    ("./**/file?.md", "-name 'file?.md'", "file?.md"),
    ("./**/[a-c]*.md", "-name '[a-c]*.md'", "[a-c]*.md"),
    ("./**/[[:digit:]]*.md", "-name '[[:digit:]]*.md'", "[0-9]*.md"),
]


def specs_for(glob, test, name):
    return [
        Native("find", ("find",), FIND.format(test=test)),
        fd(f"--glob '{name}'"),
        rg(f"-g '{name}' -g '!.*'"),
        globstar(glob),
    ]


FANOUT = 8


def name(i):
    """Item `i`'s file name: by `i % 8`, one starting `a`, `b`, `c`, `d`, a
    digit or `x`, or `file` plus one or two characters. Every ninth is a
    `.txt`."""
    ext = "txt" if i % 9 == 0 else "md"
    k = i // 8 % 10
    stem = ("a", "b", "c", "d", None, None, "7", "x")[i % 8]
    if i % 8 == 4:
        return f"file{k}.{ext}"
    if i % 8 == 5:
        return f"file{k}{k}.{ext}"
    return f"{stem}{i}.{ext}"


def folder(i):
    """Eighty items per directory, so the `file` names, ten per kind, stay
    unique."""
    m = i // 80
    return f"x{m % FANOUT}/y{m // FANOUT % FANOUT}/z{m // FANOUT**2}"


def build_tree(root, lo, hi):
    for i in range(lo, hi):
        path = root / folder(i) / name(i)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.touch()


def matched(n, glob):
    def hit(i):
        file = name(i)
        if not file.endswith(".md"):
            return False
        if glob == "./**/{a,b}*.md":
            return file[0] in "ab"
        if glob == "./**/file?.md":
            return len(file) == len("file0.md") and file.startswith("file")
        if glob == "./**/[a-c]*.md":
            return file[0] in "abc"
        return file[0].isdigit()

    return sum(1 for i in range(n) if hit(i))


def describe_matcher_glob_speed_of_light():
    @pytest.fixture(scope="module")
    def tree(tmp_path_factory):
        root = tmp_path_factory.mktemp("matchers") / "tree"
        root.mkdir()
        try:
            n = grow_until_native_takes_a_second(
                lambda lo, hi: build_tree(root, lo, hi),
                baseline(shell_natives(root, specs_for(*CASES[0]))),
                start=100_000,
                ceiling=2**22,
            )
            yield root, n, startup_seconds()
        finally:
            shutil.rmtree(root, ignore_errors=True)

    @pytest.mark.parametrize(("glob", "test", "name"), CASES)
    def it_matches_native_files_within_the_bar(tree, glob, test, name):
        specs = specs_for(glob, test, name)
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
