"""Integration: run each console script for real against a stub endpoint.

Each hook runs as a real subprocess (the module's `main`) against the local
`stub_server`; no dirsql binary and nothing mocked inside the script. Asserts
the stdout payload obeys the command-hook contract:
- on-file  -> a JSON array of row objects carrying a JSON-text embedding;
- pre-query -> a single runnable SQL string.
"""

import json
import os
import subprocess
import sys

_SRC = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "src")
)


def _run(module, arg, base_url):
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
            f"import sys; from dirsql_plugin_embeddings.{module} import main; "
            "sys.exit(main())",
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

        payload = _run("on_file", str(note), stub_server)
        rows = json.loads(payload)

        assert isinstance(rows, list) and len(rows) == 1
        (row,) = rows
        assert row["path"] == str(note)
        assert row["text"] == "Cook the pasta with garlic."
        vector = json.loads(row["embedding"])
        assert isinstance(vector, list)
        # pasta=1, cook=1, garlic=1 -> keyword-count vector from the stub.
        assert vector[:3] == [1.0, 1.0, 1.0]


def describe_pre_query_script():
    def it_prints_runnable_nearest_neighbor_sql(stub_server):
        payload = _run("pre_query", '{"q": "how do I cook pasta?"}', stub_server)

        assert payload.startswith("SELECT path, ROUND(vec_distance_cosine(embedding, '")
        assert "FROM documents ORDER BY distance LIMIT 3" in payload
        needle = payload.split("'")[1]
        # cook=1, pasta=1 embedded from the question.
        assert json.loads(needle)[:2] == [1.0, 1.0]
