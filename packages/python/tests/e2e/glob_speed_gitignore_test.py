"""Glob speed: `'./**'` inside a git repo never reads what `.gitignore` drops.

A repo whose root `.gitignore` excludes `build/` and `*.log`, and whose
`pkg/.gitignore` excludes `out/`. The ignored trees are as large as the
visible ones, so a walk that read them would pay for it. Some visible files
are committed and the rest left untracked, so both kinds count. Native is
`git ls-files --cached --others --exclude-standard`, the reference for what a
repo ignores, less the dot-named paths `'./**'` hides. No mocks: real console
script, real process, real filesystem, real git.
"""

from __future__ import annotations

import shutil
import subprocess

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

GLOB = "./**"
NATIVE = (
    "git ls-files --cached --others --exclude-standard | grep -Ev '(^|/)\\.'"
)
FANOUT = 8
GIT = ["git", "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]


def git(root, *args, stdin=None):
    subprocess.run(
        [*GIT, *args], cwd=root, input=stdin, text=True, check=True, capture_output=True
    )


def item(i):
    """Item `i`'s path and whether git keeps it. A quarter sit under the
    ignored `build/`, an eighth under `pkg`'s ignored `out/`, and every tenth
    of the rest is an ignored `.log`."""
    j = i // 8
    nest = f"x{j % FANOUT}/y{j // FANOUT % FANOUT}/z{j // FANOUT**2 % FANOUT}"
    area = i % 8
    name = f"n{i}.log" if i % 10 == 0 else f"n{i}.md"
    if area in (2, 6):
        return f"build/{nest}/{name}", False
    if area == 3:
        return f"pkg/{nest}/out/{name}", False
    if area == 7:
        return f"pkg/{nest}/{name}", not name.endswith(".log")
    return f"src/{nest}/{name}", not name.endswith(".log")


def build_tree(root, lo, hi):
    """Items `lo` through `hi - 1`; every third kept file is committed."""
    if lo == 0:
        git(root, "init", "-q")
        (root / ".gitignore").write_text("build/\n*.log\n")
        (root / "pkg").mkdir()
        (root / "pkg" / ".gitignore").write_text("out/\n")
    committed = []
    for i in range(lo, hi):
        rel, kept = item(i)
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.touch()
        if kept and i % 3 == 0:
            committed.append(rel)
    git(root, "update-index", "--add", "--stdin", stdin="\n".join(committed) + "\n")
    git(root, "commit", "-q", "--no-verify", "-m", f"items {lo}-{hi}")


def kept(n):
    return sum(1 for i in range(n) if item(i)[1])


def native(root):
    proc, seconds = timed(["bash", "-c", NATIVE], root, timeout=600)
    assert proc.returncode == 0, proc.stderr
    return sorted(proc.stdout.splitlines()), seconds


def describe_gitignore_glob_speed_of_light():
    @pytest.fixture
    def root(tmp_path):
        tree = tmp_path / "repo"
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
        assert len(expected) == kept(n)

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
