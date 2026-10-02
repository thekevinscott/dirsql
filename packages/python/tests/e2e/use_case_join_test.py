"""Use case 2, the arXiv sidecar layout: a join of two path tables.

One folder per paper holding `abstract.md` and `title.md`; the question is
every paper that has both, with its title and the abstract's size. Native is
two `find` listings, one through `awk` for the title text, sorted and joined
with `join(1)`. dirsql joins the two path tables on `dir` through the real
launcher. No mocks: real console script, real process, real filesystem.
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

COLUMNS = ("dir", "title", "abstract_bytes")
QUERY = """
SELECT a.dir, t.content AS title, a.size AS abstract_bytes
FROM './papers/*/abstract.md' a
JOIN './papers/*/title.md' t ON t.dir = a.dir
ORDER BY a.dir
"""

NATIVE = r"""
set -e
export LC_ALL=C
tab="$(printf '\t')"
join -t "$tab" \
  <(find papers -name title.md \
      -exec awk -v OFS='\t' '{ split(FILENAME, p, "/"); print p[1] "/" p[2], $0 }' {} + \
      | sort) \
  <(find papers -name abstract.md -printf '%h\t%s\n' | sort)
"""

SENTENCE = (
    "We study the problem from first principles and report a method that "
    "improves on the baseline across every setting we tried. "
)


def build_papers(root, lo, hi):
    """Paper folders `lo` through `hi - 1`. Every seventh lacks a title and
    every eleventh an abstract, so the join has to drop rows rather than
    pair everything."""
    papers = root / "papers"
    for i in range(lo, hi):
        folder = papers / f"2609.{i:06d}"
        folder.mkdir(parents=True)
        if i % 7:
            (folder / "title.md").write_text(f"Paper {i}: on the {i % 13}th method")
        if i % 11:
            (folder / "abstract.md").write_text(SENTENCE * (1 + i % 5))


def native_rows(proc):
    assert proc.returncode == 0, proc.stderr
    rows = []
    for line in proc.stdout.splitlines():
        folder, title, size = line.split("\t")
        rows.append((folder, title, int(size)))
    return rows


def describe_join_speed_of_light():
    @pytest.fixture
    def root(tmp_path):
        tree = tmp_path / "arxiv"
        try:
            yield tree
        finally:
            shutil.rmtree(tree, ignore_errors=True)

    @pytest.mark.xfail(
        strict=True, reason="path-table join is an unindexed nested loop"
    )
    def it_matches_native_rows_within_the_bar(root):
        startup = startup_seconds()

        def native():
            proc, seconds = timed(["bash", "-c", NATIVE], root, timeout=600)
            return native_rows(proc), seconds

        n = grow_until_native_takes_a_second(
            lambda lo, hi: build_papers(root, lo, hi),
            native,
            start=10_000,
            ceiling=2**20,
        )
        expected, native_seconds = best_of(native, "native")
        assert len(expected) == sum(1 for i in range(n) if i % 7 and i % 11)

        def dirsql():
            proc, seconds = timed(
                [cli(), "query", QUERY],
                root,
                timeout=dirsql_timeout(native_seconds),
            )
            return dirsql_rows(proc, COLUMNS), seconds

        actual, dirsql_seconds = best_of(
            dirsql, "dirsql", hopeless_seconds(native_seconds, startup)
        )

        assert actual == expected
        assert_speed_of_light("join", native_seconds, dirsql_seconds, startup)
