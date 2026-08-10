"""E2E: the shipped fragment loads sqlite-vec and does nothing else.

Drives the real launcher (`dirsql.cli.main:main`, in-process core) with the
plugin's packaged ``dirsql.toml``, nothing mocked: sqlite-vec's SQL functions
must be available, no ``documents`` table may exist, and querying files must
work with no embedding endpoint configured and no per-file hook subprocesses.

Run under an environment that has ``dirsql`` and this plugin installed, e.g.:

    uv run --with-editable . --with-editable ../../packages/python \
        python -m pytest tests/e2e -q
"""

import json
import os
import subprocess
import sys
from importlib import resources

import pytest

_FRAGMENT = str(resources.files("dirsql_plugin_embeddings").joinpath("dirsql.toml"))


def _run(query, cwd):
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
        timeout=120,
    )


def describe_extension_only_fragment():
    @pytest.fixture
    def notes(tmp_path):
        (tmp_path / "pasta.md").write_text("Cook the pasta.", encoding="utf-8")
        (tmp_path / "tomatoes.md").write_text("Plant tomatoes.", encoding="utf-8")
        return tmp_path

    def it_loads_sqlite_vec(notes):
        result = _run(
            "SELECT vec_distance_cosine('[1, 0]', '[0, 1]') AS d", notes
        )
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        (row,) = json.loads(result.stdout)
        assert row["d"] == 1.0

    def it_declares_no_documents_table(notes):
        result = _run("SELECT * FROM documents", notes)
        assert result.returncode != 0, f"stdout={result.stdout!r}"
        assert "no such table: documents" in result.stderr, result.stderr

    def it_queries_files_with_no_endpoint_configured(notes):
        result = _run("SELECT path FROM './*.md' ORDER BY path", notes)
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        names = sorted(
            os.path.basename(row["path"]) for row in json.loads(result.stdout)
        )
        assert names == ["pasta.md", "tomatoes.md"]
