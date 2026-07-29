"""Integration: run each console script for real against a stub endpoint.

Each hook runs as a real subprocess against the local `stub_server`; no dirsql
binary and nothing mocked inside the script. The subprocess imports the hook
exactly as its `[project.scripts]` target names it, so the import path each
case exercises -- `on_file:on_file` resolving through the `on_file` package's
barrel -- is the one a `pip install` puts on PATH. Asserts the stdout payload
obeys the command-hook contract:
- on-file  -> a JSON array of row objects carrying a JSON-text embedding;
- pre-query -> a single runnable SQL string.
"""

import json
import os
import subprocess
import sys

_SRC = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "src"))

# Two `garlic` hits and no other keyword, so the stub's count vector pins that
# the row's text is extracted PDF text rather than the file's raw bytes.
_PDF_TEXT = "Garlic confit: roast whole garlic cloves slowly in olive oil."


def _run(target, arg, base_url):
    module, attr = target.split(":")
    env = {
        **os.environ,
        "PYTHONPATH": _SRC + os.pathsep + os.environ.get("PYTHONPATH", ""),
        "DIRSQL_EMBEDDINGS_BASE_URL": base_url,
        "DIRSQL_EMBEDDINGS_MODEL": "stub-model",
        "DIRSQL_EMBEDDINGS_API_KEY": "stub-key",
    }
    proc = subprocess.run(
        [
            sys.executable,
            "-c",
            f"import sys; from dirsql_plugin_embeddings.{module} import {attr}; "
            f"sys.exit({attr}())",
            arg,
        ],
        capture_output=True,
        text=True,
        env=env,
        timeout=30,
    )
    assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
    return [line for line in proc.stdout.splitlines() if line.strip()][-1]


def describe_on_file_script():
    def it_prints_a_row_array_with_a_json_embedding(tmp_path, stub_server):
        note = tmp_path / "pasta.md"
        note.write_text("Cook the pasta with garlic.", encoding="utf-8")

        payload = _run("on_file:on_file", str(note), stub_server)
        rows = json.loads(payload)

        assert isinstance(rows, list) and len(rows) == 1
        (row,) = rows
        assert row["path"] == str(note)
        assert row["text"] == "Cook the pasta with garlic."
        vector = json.loads(row["embedding"])
        assert isinstance(vector, list)
        # pasta=1, cook=1, garlic=1 -> keyword-count vector from the stub.
        assert vector[:3] == [1.0, 1.0, 1.0]

    def it_embeds_the_extracted_text_of_a_real_pdf(tmp_path, stub_server, make_pdf):
        paper = tmp_path / "garlic.pdf"
        paper.write_bytes(make_pdf(_PDF_TEXT))

        payload = _run("on_file:on_file", str(paper), stub_server)
        (row,) = json.loads(payload)

        assert row["path"] == str(paper)
        assert row["text"] == _PDF_TEXT
        # pasta=0, cook=0, garlic=2 -> the text came out of the PDF, not bytes.
        assert json.loads(row["embedding"])[:3] == [0.0, 0.0, 2.0]

    def it_routes_an_uppercase_pdf_extension_too(tmp_path, stub_server, make_pdf):
        paper = tmp_path / "GARLIC.PDF"
        paper.write_bytes(make_pdf(_PDF_TEXT))

        (row,) = json.loads(_run("on_file:on_file", str(paper), stub_server))

        assert row["text"] == _PDF_TEXT


def describe_pre_query_script():
    def it_prints_runnable_nearest_neighbor_sql(stub_server):
        payload = _run("pre_query:main", '{"q": "how do I cook pasta?"}', stub_server)

        assert payload.startswith("SELECT path, ROUND(vec_distance_cosine(embedding, '")
        assert "FROM documents ORDER BY distance LIMIT 3" in payload
        needle = payload.split("'")[1]
        # cook=1, pasta=1 embedded from the question.
        assert json.loads(needle)[:2] == [1.0, 1.0]
