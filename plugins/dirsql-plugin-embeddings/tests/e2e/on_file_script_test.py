"""E2E: the installed `dirsql-embeddings-on-file` console script, nothing mocked.

`semantic_search_test.py` drives the hook the way dirsql does -- as an opaque
command string in the fragment -- so a broken `[project.scripts]` target
surfaces there only as an unexplained empty table. These cases pin the script
itself: the executable a `pip install` puts on PATH, run as a real process over
a real file, and the callable that executable dispatches to. Only the embedding
endpoint is a local stub HTTP server (a hosted model is not reachable from CI
or a clean checkout).

Together they hold the `on_file` package's barrel to its contract: the entry
point declared in pyproject.toml resolves through `on_file/__init__.py` to the
same `on_file` callable importable from the package, so splitting the module
into one-function files can never silently orphan the shipped command.

Run under an environment that has this plugin installed, e.g.:

    uv run --with sqlite-vec --with-editable . \
        --with-editable ../../packages/python python -m pytest tests/e2e
"""

import json
import os
import shutil
import subprocess
import sys
from importlib.metadata import entry_points

from dirsql_plugin_embeddings.on_file import on_file

_SCRIPT_NAME = "dirsql-embeddings-on-file"


def _installed_script():
    # The venv's bin dir is not necessarily on PATH when pytest is invoked by
    # absolute path, so look beside the interpreter first.
    beside = os.path.join(os.path.dirname(sys.executable), _SCRIPT_NAME)
    return beside if os.path.exists(beside) else shutil.which(_SCRIPT_NAME)


def describe_on_file_console_script():
    def it_dispatches_to_the_callable_re_exported_by_the_package():
        (declared,) = [
            entry
            for entry in entry_points(group="console_scripts")
            if entry.name == _SCRIPT_NAME
        ]
        assert declared.load() is on_file

    def it_embeds_a_real_file_into_a_row_array(tmp_path, stub_server):
        script = _installed_script()
        assert script, f"{_SCRIPT_NAME} not on PATH; install the plugin first"

        note = tmp_path / "pasta.md"
        note.write_text("Cook the pasta with garlic.", encoding="utf-8")

        result = subprocess.run(
            [script, str(note)],
            capture_output=True,
            text=True,
            env={
                **os.environ,
                "DIRSQL_EMBEDDINGS_BASE_URL": stub_server,
                "DIRSQL_EMBEDDINGS_MODEL": "stub-model",
                "DIRSQL_EMBEDDINGS_API_KEY": "stub-key",
            },
            timeout=60,
        )

        assert result.returncode == 0, (
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        (row,) = json.loads(result.stdout)
        assert row["path"] == str(note)
        assert row["text"] == "Cook the pasta with garlic."
        # pasta=1, cook=1, garlic=1 -> keyword-count vector from the stub.
        assert json.loads(row["embedding"])[:3] == [1.0, 1.0, 1.0]
