"""Use case 2 through the plugin: embedding every abstract in a corpus.

The native reference is the model itself: one Python process that loads the
same model2vec model and batch-encodes every file's text. dirsql embeds each
file through `embed()` and the real worker process, cold cache every run.
The vectors must be identical and dirsql's wall time, after subtracting the
launcher's startup, within ten percent of the batch encode. Nothing mocked:
real launcher, real worker spawn, real model2vec inference on the on-disk
model from conftest.
"""

from __future__ import annotations

import json
import os
import shutil
import sys
from importlib import resources

import pytest

from .speed_of_light import (
    assert_speed_of_light,
    best_of,
    dirsql_rows,
    dirsql_timeout,
    grow_until_native_takes_a_second,
    hopeless_seconds,
    startup_seconds,
    timed,
)

_FRAGMENT = str(resources.files("dirsql_plugin_embeddings").joinpath("dirsql.toml"))
LAUNCHER = [
    sys.executable,
    "-c",
    "import sys; from dirsql.cli.main import main; sys.exit(main())",
    "--no-plugin",
]
COLUMNS = ("path", "emb")

ENCODE = """\
import json, os, sys
from model2vec import StaticModel

model = StaticModel.from_pretrained(sys.argv[1])
names = sorted(os.listdir("abstracts"))
texts = [open(os.path.join("abstracts", name), encoding="utf-8").read() for name in names]
for name, vector in zip(names, model.encode(texts, show_progress_bar=False)):
    print(name, json.dumps([float(c) for c in vector]), sep="\\t")
"""

VOCABULARY = ("hello", "world")


def build_abstracts(root, lo, hi):
    """Abstracts `lo` through `hi - 1`, each a different run of the model's
    two known words so no two neighbours embed alike."""
    folder = root / "abstracts"
    if lo == 0:
        folder.mkdir(parents=True)
        (root / "encode.py").write_text(ENCODE)
    for i in range(lo, hi):
        words = [VOCABULARY[(i >> bit) & 1] for bit in range(3 + i % 9)]
        (folder / f"{i:07d}.md").write_text(" ".join(words), encoding="utf-8")


def native_rows(proc):
    assert proc.returncode == 0, proc.stderr
    rows = []
    for line in proc.stdout.splitlines():
        name, vector = line.split("\t")
        rows.append((name, json.loads(vector)))
    return rows


def describe_embeddings_speed_of_light():
    @pytest.fixture
    def root(tmp_path):
        tree = tmp_path / "corpus"
        try:
            yield tree
        finally:
            shutil.rmtree(tree, ignore_errors=True)

    @pytest.mark.xfail(
        strict=True,
        reason="embed() makes one worker round trip per value instead of a batch",
    )
    def it_matches_the_batch_encode_within_the_bar(root, tiny_model, tmp_path):
        startup = startup_seconds([*LAUNCHER, "--help"], cwd=tmp_path)

        def native():
            proc, seconds = timed(
                [sys.executable, "encode.py", tiny_model], root, timeout=600
            )
            return native_rows(proc), seconds

        grow_until_native_takes_a_second(
            lambda lo, hi: build_abstracts(root, lo, hi),
            native,
            start=4_096,
            ceiling=2**18,
        )
        expected, native_seconds = best_of(native, "native")

        query = (
            "SELECT path, embed(content, '" + tiny_model + "') AS emb"
            " FROM './abstracts/*.md' ORDER BY path"
        )
        cold_runs = iter(range(10))

        def dirsql():
            cache_home = tmp_path / f"xdg-cache-{next(cold_runs)}"
            proc, seconds = timed(
                [*LAUNCHER, "query", query, "--config", _FRAGMENT],
                root,
                timeout=dirsql_timeout(native_seconds),
                env={**os.environ, "XDG_CACHE_HOME": str(cache_home)},
            )
            rows = dirsql_rows(proc, COLUMNS)
            return [
                (os.path.basename(path), json.loads(emb)) for path, emb in rows
            ], seconds

        actual, dirsql_seconds = best_of(
            dirsql, "dirsql", hopeless_seconds(native_seconds, startup)
        )

        assert actual == expected
        assert_speed_of_light("embeddings", native_seconds, dirsql_seconds, startup)
