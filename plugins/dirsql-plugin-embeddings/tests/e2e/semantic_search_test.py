"""E2E: the full semantic-search loop, nothing mocked.

Drives the real Python launcher (`dirsql.cli.main:main`) + the cargo-built
`dirsql` binary with this plugin *installed* (its console scripts on PATH and
`sqlite_vec` importable). The launcher resolves the `sqlite_vec` package-name
extension to its loadable, loads it, runs `on-file` over a fixture dir to embed
each note, then routes the query body through `pre-query` to nearest-neighbor
SQL ordered by `vec_distance_cosine`. Only the embedding endpoint is a local
stub (a real HTTP server), because CI/e2e cannot call a hosted model --
everything else is the shipped stack.

The plugin's `dirsql.toml` is passed with an explicit `--config` (and
`--no-plugin`), NOT auto-discovery: driving the fragment directly exercises
the plugin's real machinery (extension + both hooks + vec ranking) without
depending on the discovery scan. The discovery path -- including package-name
extension resolution for discovery-injected `-c` fragments (#754) -- is
covered by the core's own plugin_discovery_test / plugin_extension_discovery_test.

Run under an environment that has `dirsql`, `sqlite_vec`, and this plugin
installed together, e.g.:

    uv run --with sqlite-vec --with-editable . \
        --with-editable ../../packages/python python -m pytest tests/e2e

and with the binary built: `cargo build -p dirsql --features cli`.
"""

import json
import os
import shutil
import subprocess
import sys
from importlib import resources

import pytest

import dirsql as _dirsql_pkg

_FRAGMENT = str(resources.files("dirsql_plugin_embeddings").joinpath("dirsql.toml"))

_REPO_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..")
)
_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "dirsql")
_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "dirsql")
_BINARY = _RELEASE if os.path.exists(_RELEASE) else _DEBUG

_BINARY_STAGE_DIR = os.path.join(os.path.dirname(_dirsql_pkg.__file__), "_binary")

_FIXTURES = {
    "pasta.md": "Cook the pasta: boil spaghetti, then toss with garlic.",
    "branches.md": "Git branches let you review code before you merge.",
    "tomatoes.md": "Plant tomato seeds and water the seedlings each morning.",
    # One per non-Markdown prose extension the fragment claims to index. Each
    # repeats a single keyword and no other, so its embedding points straight
    # down that axis and it outranks the .md note that mentions the same word
    # in passing -- which it can only do if the glob reached the file at all.
    "trays.txt": "Tomato, tomato, and more tomato.",
    "reviews.rst": "Review the review, then review again.",
}

# A PDF among the Markdown notes: it must be matched by the fragment's glob and
# read through the extension routing, or it never reaches the index at all.
# Two `garlic` hits and nothing else, so a garlic question ranks it ahead of
# pasta.md (which dilutes garlic with pasta and cook).
_PDF_FIXTURES = {
    "garlic.pdf": "Garlic confit: roast whole garlic cloves slowly in olive oil.",
}


def _run(query, data_dir, base_url):
    env = {
        **os.environ,
        "PATH": os.path.dirname(sys.executable)
        + os.pathsep
        + os.environ.get("PATH", ""),
        "DIRSQL_EMBEDDINGS_BASE_URL": base_url,
        "DIRSQL_EMBEDDINGS_MODEL": "stub-model",
        "DIRSQL_EMBEDDINGS_API_KEY": "stub-key",
    }
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
        cwd=str(data_dir),
        capture_output=True,
        text=True,
        env=env,
        timeout=120,
    )


def describe_semantic_search():
    @pytest.fixture
    def staged(tmp_path, make_pdf):
        assert os.path.exists(_BINARY), (
            f"dirsql binary not built at {_BINARY}; "
            "run `cargo build -p dirsql --features cli` first"
        )
        os.makedirs(_BINARY_STAGE_DIR, exist_ok=True)
        staged_binary = os.path.join(_BINARY_STAGE_DIR, "dirsql")
        shutil.copy(_BINARY, staged_binary)
        os.chmod(staged_binary, 0o755)

        data = tmp_path / "notes"
        data.mkdir()
        for name, text in _FIXTURES.items():
            (data / name).write_text(text, encoding="utf-8")
        for name, text in _PDF_FIXTURES.items():
            (data / name).write_bytes(make_pdf(text))
        try:
            yield data
        finally:
            shutil.rmtree(_BINARY_STAGE_DIR, ignore_errors=True)

    def it_returns_the_semantically_nearest_note(staged, stub_server):
        result = _run('{"q": "how do I cook pasta?"}', staged, stub_server)
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        rows = json.loads(result.stdout)
        assert rows, f"no rows returned: {result.stdout!r}"
        assert os.path.basename(rows[0]["path"]) == "pasta.md", rows
        # Distances are ordered ascending (nearest first).
        distances = [row["distance"] for row in rows]
        assert distances == sorted(distances), rows

    def it_indexes_a_plain_text_note(staged, stub_server):
        result = _run('{"q": "tomato"}', staged, stub_server)
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        rows = json.loads(result.stdout)
        assert os.path.basename(rows[0]["path"]) == "trays.txt", rows

    def it_indexes_a_restructuredtext_note(staged, stub_server):
        result = _run('{"q": "review"}', staged, stub_server)
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        rows = json.loads(result.stdout)
        assert os.path.basename(rows[0]["path"]) == "reviews.rst", rows

    def it_answers_a_different_question_with_a_different_note(staged, stub_server):
        result = _run('{"q": "reviewing code on git"}', staged, stub_server)
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        rows = json.loads(result.stdout)
        assert os.path.basename(rows[0]["path"]) == "branches.md", rows

    def it_returns_a_pdf_backed_note_as_the_nearest_hit(staged, stub_server):
        result = _run('{"q": "how do I roast garlic?"}', staged, stub_server)
        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        rows = json.loads(result.stdout)
        assert rows, f"no rows returned: {result.stdout!r}"
        # The PDF only ranks if the glob matched it AND its text was extracted:
        # unindexed, or indexed as raw bytes, and pasta.md wins on `garlic`.
        assert os.path.basename(rows[0]["path"]) == "garlic.pdf", rows
