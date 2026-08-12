"""E2E: the one-liner search CLI, nothing mocked.

Spawns the real ``dirsql-plugin-embeddings`` console script against a fixture
directory: real click dispatch, the real dirsql SDK running the generated
search SQL against real SQLite with the packaged config fragment (real
sqlite-vec, the real ``[[dirsql.function]]`` worker spawn), real model2vec
inference via the on-disk model from conftest passed through ``--model``, and
real cachetta writes under a temp ``XDG_CACHE_HOME``.

Run under an environment that has this plugin and dirsql installed, e.g.:

    uv run --with-editable ../../packages/python python -m pytest tests/e2e -q
"""

import os
import shutil
import subprocess

import pytest


def script():
    path = shutil.which("dirsql-plugin-embeddings")
    assert path, "console script dirsql-plugin-embeddings must be on PATH"
    return path


def _search(args, cwd, cache_home):
    return subprocess.run(
        [script(), *args],
        cwd=str(cwd),
        capture_output=True,
        text=True,
        env={**os.environ, "XDG_CACHE_HOME": str(cache_home)},
        timeout=300,
    )


@pytest.fixture
def tree(tmp_path):
    notes = tmp_path / "notes"
    notes.mkdir()
    # tiny_model: hello -> [1, 0], world -> [0, 1].
    (notes / "greeting.txt").write_text("hello", encoding="utf-8")
    (notes / "planet.txt").write_text("world", encoding="utf-8")
    return tmp_path


def describe_search_cli():
    def it_ranks_globbed_files_by_distance_to_the_query(
        tree, tiny_model, cache_home
    ):
        result = _search(
            ["./notes/*.txt", "world", "--model", tiny_model], tree, cache_home
        )
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        lines = result.stdout.splitlines()
        assert len(lines) == 2
        paths = [os.path.basename(line.split("\t")[0]) for line in lines]
        distances = [float(line.split("\t")[1]) for line in lines]
        assert paths == ["planet.txt", "greeting.txt"]
        assert distances[0] == pytest.approx(0.0)
        assert distances[1] == pytest.approx(1.0)
        assert distances == sorted(distances)

    def it_limits_results_to_k_and_accepts_a_bare_glob(
        tree, tiny_model, cache_home
    ):
        result = _search(
            ["notes/*.txt", "world", "-k", "1", "--model", tiny_model],
            tree,
            cache_home,
        )
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        lines = result.stdout.splitlines()
        assert len(lines) == 1
        assert os.path.basename(lines[0].split("\t")[0]) == "planet.txt"

    def it_fails_loudly_when_the_glob_matches_no_files(
        tree, tiny_model, cache_home
    ):
        """Silent empty output is indistinguishable from "searched and found
        nothing relevant" (#816). A glob that matches nothing is a mistake the
        user can fix, so say so and exit nonzero."""
        result = _search(
            ["./nowhere/*.txt", "world", "--model", tiny_model],
            tree,
            cache_home,
        )

        assert result.returncode != 0, f"stdout={result.stdout!r}"
        assert result.stdout == ""
        assert "no files matched './nowhere/*.txt'" in result.stderr
        assert str(tree) in result.stderr, "the message names where it looked"
        assert "Traceback" not in result.stderr

    def it_errors_actionably_when_the_query_is_missing(tree, cache_home):
        result = _search(["./notes/*.txt"], tree, cache_home)
        assert result.returncode == 2
        assert "Missing argument" in result.stderr
        assert "QUERY" in result.stderr
        assert "Traceback" not in result.stderr
