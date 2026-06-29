"""E2E tests for `dirsql interpret` -- the long-running native config
helper (#196).

Each test spawns the real `dirsql` console script as a subprocess and
talks NDJSON over stdin/stdout. No mocking of any kind (real process,
real filesystem); the interpret loop's logic is unit-tested in
`dirsql/cli/interpret/run_test.py`. Subprocess plumbing lives in
`interpret_subprocess.py`.

NDJSON protocol (per #196):

  handshake (helper -> caller, once on startup):
    {"type": "config", "state": <vars(app)>}

  extract request (caller -> helper):
    {"type": "extract", "id": <int>, "table": "<name>", "path": "<abs>"}

  extract response (helper -> caller):
    {"type": "result", "id": <int>, "ok": true,  "rows": [...]}
    {"type": "result", "id": <int>, "ok": false, "error": "<msg>"}
"""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path

from tests.e2e.interpret_subprocess import (
    cli_argv,
    readline,
    send,
    shutdown,
    spawn,
)

FIXTURE_DIR = Path(__file__).parent / "__fixtures__" / "interpret"
HAPPY_CONFIG = FIXTURE_DIR / "dirsql.config.py"
RAISES_CONFIG = FIXTURE_DIR / "dirsql.config_raises.py"
NO_APP_CONFIG = FIXTURE_DIR / "dirsql.config_no_app.py"
NO_ROOT_CONFIG = FIXTURE_DIR / "dirsql.config_no_root.py"
NESTED_CONFIG = FIXTURE_DIR / "dirsql.config_nested.py"
ALPHA_PATH = FIXTURE_DIR / "data" / "a" / "meta.json"


def describe_dirsql_interpret():
    def describe_handshake():
        def it_emits_a_config_message_whose_state_equals_vars_app():
            """`state` is the same dict shape `vars(app)` produces."""
            proc = spawn(HAPPY_CONFIG)
            try:
                assert json.loads(readline(proc)) == {
                    "type": "config",
                    "state": {
                        "root": str(FIXTURE_DIR / "data"),
                        "tables": [
                            {
                                "ddl": "CREATE TABLE papers (title TEXT)",
                                "glob": "**/meta.json",
                                "strict": False,
                            }
                        ],
                        "ignore": [],
                        "persist": False,
                        "persist_path": None,
                        "extensions": [],
                    },
                }
            finally:
                shutdown(proc)

        def it_defaults_root_to_cwd_when_the_app_omits_root(tmp_path):
            """A config that omits `root` reports the helper's cwd as root --
            not null, and not the config file's directory."""
            cwd = os.path.realpath(tmp_path)
            proc = subprocess.Popen(
                [*cli_argv(), "interpret", str(NO_ROOT_CONFIG)],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                bufsize=1,
                cwd=cwd,
            )
            try:
                state = json.loads(readline(proc))["state"]
                assert state["root"] == cwd
            finally:
                shutdown(proc)

    def describe_extract():
        def it_returns_ok_true_with_the_rows_extract_produced():
            proc = spawn(HAPPY_CONFIG)
            try:
                readline(proc)  # drain handshake
                send(
                    proc,
                    {
                        "type": "extract",
                        "id": 1,
                        "table": "papers",
                        "path": str(ALPHA_PATH),
                    },
                )
                assert json.loads(readline(proc)) == {
                    "type": "result",
                    "id": 1,
                    "ok": True,
                    "rows": [{"title": "Alpha"}],
                }
            finally:
                shutdown(proc)

        def it_returns_ok_false_when_the_user_extract_raises():
            proc = spawn(RAISES_CONFIG)
            try:
                readline(proc)  # drain handshake
                send(
                    proc,
                    {
                        "type": "extract",
                        "id": 7,
                        "table": "papers",
                        "path": str(ALPHA_PATH),
                    },
                )
                response = json.loads(readline(proc))
                # `error` is dynamic; verify the message, then assert the
                # rest of the shape exactly.
                assert "synthetic extract failure" in response.pop("error", "")
                assert response == {"type": "result", "id": 7, "ok": False}
            finally:
                shutdown(proc)

        def it_returns_ok_false_when_the_request_names_an_unknown_table():
            proc = spawn(HAPPY_CONFIG)
            try:
                readline(proc)  # drain handshake
                send(
                    proc,
                    {
                        "type": "extract",
                        "id": 3,
                        "table": "nonexistent",
                        "path": str(ALPHA_PATH),
                    },
                )
                response = json.loads(readline(proc))
                assert "nonexistent" in response.pop("error", "")
                assert response == {"type": "result", "id": 3, "ok": False}
            finally:
                shutdown(proc)

    def describe_startup():
        def it_exits_nonzero_with_clean_stderr_when_the_config_has_no_app():
            proc = subprocess.Popen(
                [*cli_argv(), "interpret", str(NO_APP_CONFIG)],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            stdout, stderr = proc.communicate(timeout=10)
            assert proc.returncode != 0, (
                f"expected non-zero exit; stdout={stdout!r}, stderr={stderr!r}"
            )
            # "Clean": a single human-readable line, not a Python traceback.
            assert "Traceback" not in stderr
            assert "app" in stderr.lower()

        def it_exits_nonzero_with_clean_stderr_when_the_app_sets_config():
            """A native config whose `app` delegates to another config file is
            rejected: nested config loading is not supported."""
            proc = subprocess.Popen(
                [*cli_argv(), "interpret", str(NESTED_CONFIG)],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            stdout, stderr = proc.communicate(timeout=10)
            assert proc.returncode != 0, (
                f"expected non-zero exit; stdout={stdout!r}, stderr={stderr!r}"
            )
            assert "Traceback" not in stderr
            assert "config" in stderr.lower()
