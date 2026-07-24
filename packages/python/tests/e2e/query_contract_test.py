"""CLI e2e: the documented `POST /query` contract through the real launcher +
binary (docs/reference/http-api.md).

Boots the Python launcher (`dirsql.cli.main:main`) over a real `.dirsql.toml`
and asserts the documented query contract end to end: a JSON array of row
objects on success, and each documented failure class -- malformed JSON body,
missing/empty `sql`, SQL errors, the read-only rule, and the `_dirsql_*`
internal-table denial (all `400` with a JSON `{"error": ...}` body), plus
`405` for `GET /query`. No mocks: real launcher, real binary, real process,
real filesystem.

The Rust tiers already pin this contract in-crate, but the language packages
ship the binary, so each SDK's e2e suite must pin the HTTP surface it ships.
"""

import json
import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

import pytest

_REPO_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..")
)
_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "dirsql")
_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "dirsql")
_BINARY = _RELEASE if os.path.exists(_RELEASE) else _DEBUG

# Where the launcher's `binary_path()` looks: `<dirsql package>/_binary/dirsql`.
import dirsql as _dirsql_pkg  # noqa: E402

_BINARY_STAGE_DIR = os.path.join(os.path.dirname(_dirsql_pkg.__file__), "_binary")

# An `on-file` hook emitting `path`, derived from the file path (`{path}`)
# relative to the scan root (`{root}`).
_HOOK_PATH = r"""on-file = '''sh -c 'rel=${1#"$2"/}; printf "[{\"path\":\"%s\"}]" "$rel"' sh {path} {root}'''"""


def _free_port():
    with socket.socket() as s:
        s.bind(("", 0))
        return s.getsockname()[1]


def _wait_for_server(proc, port, timeout=5.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            return False
        try:
            urllib.request.urlopen(f"http://localhost:{port}/query", timeout=0.5)
            return True
        except urllib.error.HTTPError:
            return True
        except Exception:
            time.sleep(0.05)
    return False


def _post_raw(port, body):
    """POST raw bytes to /query; return (status, parsed-JSON body)."""
    req = urllib.request.Request(
        f"http://localhost:{port}/query",
        data=body,
        headers={"content-type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as err:
        return err.code, json.loads(err.read())


def _post_sql(port, sql):
    return _post_raw(port, json.dumps({"sql": sql}).encode())


def describe_query_contract():
    @pytest.fixture
    def server(tmp_path):
        assert os.path.exists(_BINARY), (
            f"dirsql binary not built at {_BINARY}; "
            "run `cargo build -p dirsql --features cli` first"
        )
        # Stage the binary where the launcher's `binary_path()` looks.
        os.makedirs(_BINARY_STAGE_DIR, exist_ok=True)
        staged_binary = os.path.join(_BINARY_STAGE_DIR, "dirsql")
        shutil.copy(_BINARY, staged_binary)
        os.chmod(staged_binary, 0o755)

        root = tmp_path / "data"
        root.mkdir()
        (root / "a.txt").write_text("hello")
        cfg = root / ".dirsql.toml"
        cfg.write_text(
            '[[table]]\nddl = "CREATE TABLE files (path TEXT)"\nglob = "*.txt"\n'
            f"{_HOOK_PATH}\n"
        )

        port = _free_port()
        proc = subprocess.Popen(
            [
                sys.executable,
                "-c",
                "import sys; from dirsql.cli.main import main; sys.exit(main())",
                "--config",
                str(cfg),
                "--host",
                "127.0.0.1",
                "--port",
                str(port),
            ],
            cwd=str(root),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            ok = _wait_for_server(proc, port)
            if not ok:
                out, err = proc.communicate(timeout=5)
                raise AssertionError(
                    f"server did not start\n--- stdout ---\n{out}\n--- stderr ---\n{err}"
                )
            yield port
        finally:
            proc.terminate()
            proc.wait(timeout=5)
            shutil.rmtree(_BINARY_STAGE_DIR, ignore_errors=True)

    def it_returns_rows_as_a_json_array_of_objects(server):
        status, rows = _post_sql(server, "SELECT 1 AS one, 'x' AS s")
        assert status == 200
        assert rows == [{"one": 1, "s": "x"}]

    def it_serves_a_config_table_row_per_matched_file(server):
        status, rows = _post_sql(server, "SELECT path FROM files")
        assert status == 200
        assert len(rows) == 1
        assert rows[0]["path"].endswith("a.txt")

    def it_rejects_a_missing_sql_field_with_400(server):
        status, body = _post_raw(server, b"{}")
        assert status == 400
        assert body == {"error": "missing `sql` field"}

    def it_rejects_an_empty_sql_field_with_400(server):
        status, body = _post_sql(server, "   ")
        assert status == 400
        assert body == {"error": "`sql` must not be empty"}

    def it_rejects_a_malformed_json_body_with_400(server):
        status, body = _post_raw(server, b"not json")
        assert status == 400
        assert body["error"]

    def it_rejects_a_sql_error_with_400(server):
        status, body = _post_sql(server, "SELECT * FROM nope")
        assert status == 400
        assert "no such table" in body["error"]

    def it_rejects_a_write_statement_with_400(server):
        status, body = _post_sql(server, "DELETE FROM files")
        assert status == 400
        assert "read-only" in body["error"]

    def it_denies_internal_table_reads_with_400(server):
        # The `_dirsql_*` bookkeeping namespace is not readable through the
        # query surface; a rejected read is an error, not empty output.
        status, body = _post_sql(server, "SELECT * FROM _dirsql_internal_rows")
        assert status == 400
        assert "not authorized" in body["error"]

    def it_rejects_get_with_405(server):
        try:
            urllib.request.urlopen(f"http://localhost:{server}/query", timeout=5)
            raise AssertionError("GET /query must not succeed")
        except urllib.error.HTTPError as err:
            assert err.code == 405
            assert err.read().decode() == "method not allowed"
