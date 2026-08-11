"""E2E: the packaged [[dirsql.function]] fragment through the real launcher.

Drives the real `dirsql` CLI (`dirsql.cli.main:main`, in-process core) with the
plugin's packaged ``dirsql.toml``, nothing mocked: the core's
`[[dirsql.function]]` mechanism spawns the real `dirsql-plugin-embeddings
worker` console script, which runs real model2vec inference (the on-disk model
from conftest via the ordinary model-override argument) and writes the real
cachetta cache under a temp ``XDG_CACHE_HOME``.

Pins the epic's two end-to-end guarantees (#800 / #804):

- **Glob scoping**: a query whose glob matches a subset of files embeds
  exactly that subset — observed via the cache, which gains one entry per
  distinct (value, model) embedded and nothing for files outside the glob.
- **Zero cost when unused**: a query that never calls embed() spawns no
  worker process — observed via a PATH shim that logs each real worker spawn
  before exec'ing it (a positive control proves the instrument sees spawns).

Run under an environment that has this plugin and dirsql installed, e.g.:

    uv run --with-editable ../../packages/python python -m pytest tests/e2e -q
"""

import json
import os
import shutil
import stat
import subprocess
import sys
from importlib import resources

import pytest

_FRAGMENT = str(resources.files("dirsql_plugin_embeddings").joinpath("dirsql.toml"))


def _run(query, cwd, cache_home, path=None):
    env = {**os.environ, "XDG_CACHE_HOME": str(cache_home)}
    if path is not None:
        env["PATH"] = path
    return subprocess.run(
        [
            sys.executable,
            "-c",
            "import sys; from dirsql.cli.main import main; sys.exit(main())",
            "--no-plugin",
            "query",
            query,
            "--config",
            _FRAGMENT,
        ],
        cwd=str(cwd),
        capture_output=True,
        text=True,
        env=env,
        timeout=300,
    )


def _cache_entries(cache_home):
    embeddings = cache_home / "dirsql" / "embeddings"
    if not embeddings.is_dir():
        return 0
    return sum(1 for path in embeddings.iterdir() if path.is_file())


def describe_glob_scoping():
    @pytest.fixture
    def tree(tmp_path):
        notes = tmp_path / "notes"
        sub = notes / "sub"
        sub.mkdir(parents=True)
        (notes / "outside_a.txt").write_text("hello hello", encoding="utf-8")
        (notes / "outside_b.txt").write_text("world world", encoding="utf-8")
        (sub / "inside_a.txt").write_text("hello", encoding="utf-8")
        (sub / "inside_b.txt").write_text("world", encoding="utf-8")
        return tmp_path

    def it_embeds_exactly_the_globbed_subset(tree, tiny_model, cache_home):
        result = _run(
            "SELECT path, embed(content, '"
            + tiny_model
            + "') AS emb FROM './notes/sub/*.txt' ORDER BY path",
            tree,
            cache_home,
        )
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        rows = json.loads(result.stdout)
        assert [os.path.basename(row["path"]) for row in rows] == [
            "inside_a.txt",
            "inside_b.txt",
        ]
        assert json.loads(rows[0]["emb"]) == [1.0, 0.0]
        assert json.loads(rows[1]["emb"]) == [0.0, 1.0]
        # One cache entry per matched file's content; the files outside the
        # glob (distinct contents) would each have added another entry had
        # they been read and embedded.
        assert _cache_entries(cache_home) == 2

    def it_ranks_by_vec_distance_over_embedded_content(
        tree, tiny_model, cache_home
    ):
        result = _run(
            "SELECT path FROM (SELECT path, embed(content, '"
            + tiny_model
            + "') AS emb FROM './notes/sub/*.txt')"
            + " ORDER BY vec_distance_cosine(emb, embed('world', '"
            + tiny_model
            + "')) LIMIT 1",
            tree,
            cache_home,
        )
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        (row,) = json.loads(result.stdout)
        assert os.path.basename(row["path"]) == "inside_b.txt"


def describe_zero_cost_when_unused():
    @pytest.fixture
    def spawn_log(tmp_path):
        return tmp_path / "spawns.log"

    @pytest.fixture
    def instrumented_path(tmp_path, spawn_log):
        real = shutil.which("dirsql-plugin-embeddings")
        assert real, "console script dirsql-plugin-embeddings must be on PATH"
        shim_dir = tmp_path / "shim-bin"
        shim_dir.mkdir()
        shim = shim_dir / "dirsql-plugin-embeddings"
        shim.write_text(
            f'#!/bin/sh\necho spawned >> "{spawn_log}"\nexec "{real}" "$@"\n',
            encoding="utf-8",
        )
        shim.chmod(shim.stat().st_mode | stat.S_IXUSR)
        return f"{shim_dir}{os.pathsep}{os.environ['PATH']}"

    @pytest.fixture
    def tree(tmp_path):
        notes = tmp_path / "notes"
        notes.mkdir()
        (notes / "a.txt").write_text("hello", encoding="utf-8")
        return tmp_path

    def it_spawns_no_worker_for_a_query_without_embed(
        tree, cache_home, spawn_log, instrumented_path
    ):
        result = _run(
            "SELECT path FROM './notes/*.txt'",
            tree,
            cache_home,
            path=instrumented_path,
        )
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        (row,) = json.loads(result.stdout)
        assert os.path.basename(row["path"]) == "a.txt"
        assert not spawn_log.exists()
        assert _cache_entries(cache_home) == 0

    def it_observes_a_spawn_when_embed_is_called_positive_control(
        tree, tiny_model, cache_home, spawn_log, instrumented_path
    ):
        result = _run(
            "SELECT embed('hello', '" + tiny_model + "') AS emb",
            tree,
            cache_home,
            path=instrumented_path,
        )
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        assert spawn_log.read_text(encoding="utf-8") == "spawned\n"
