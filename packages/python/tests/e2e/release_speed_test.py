"""Speed sanity check: this build against the previous published dirsql.

One case per path category (glob walk, stat columns), each a
small generated corpus queried through both builds. dirsql is held to itself
here, never to a native: only a gross slowdown against the last release fails.
No mocks: real console scripts, real processes, real filesystem.
"""

from __future__ import annotations

import shutil
import tempfile
from pathlib import Path

import pytest

from .glob_speed_recursive_test import build_tree as build_walk_tree
from .speed_of_light import (
    Startup,
    assert_no_slowdown,
    cli,
    install_release,
    probe_seconds,
    release_ratios,
    timed,
)
from .stat_speed_test import build_tree as build_stat_tree

WALK_FILES = 100_000
STAT_FILES = 100_000
TIMEOUT = 120

WALK_QUERY = "SELECT path FROM './**/*.md'"
STAT_QUERY = "SELECT path, size, mtime FROM './**/*'"


def walk_tree(root):
    build_walk_tree(root, 0, WALK_FILES)
    return ["query", WALK_QUERY]


def stat_tree(root):
    build_stat_tree(root, 0, STAT_FILES)
    return ["query", STAT_QUERY]


def describe_release_speed_sanity():
    @pytest.fixture(scope="module")
    def release():
        with tempfile.TemporaryDirectory() as scratch:
            yield install_release(Path(scratch) / "release")

    @pytest.fixture(scope="module")
    def startup(release):
        with tempfile.TemporaryDirectory() as empty:
            return Startup(
                probe_seconds([cli(), "query", "SELECT 1"], empty),
                probe_seconds([str(release), "query", "SELECT 1"], empty),
            )

    @pytest.mark.parametrize(
        ("name", "build"),
        [("walk", walk_tree), ("stat", stat_tree)],
    )
    def it_is_not_grossly_slower_than_the_previous_release(
        name, build, release, startup, tmp_path
    ):
        root = tmp_path / name
        root.mkdir()
        try:
            argv = build(root)

            def run(binary):
                def go():
                    proc, seconds = timed([binary, *argv], root, TIMEOUT)
                    assert proc.returncode == 0, proc.stderr
                    return proc.stdout, seconds

                return go

            def measure():
                return release_ratios(run(cli()), run(str(release)), startup)

            assert_no_slowdown(name, measure)
        finally:
            shutil.rmtree(root, ignore_errors=True)
