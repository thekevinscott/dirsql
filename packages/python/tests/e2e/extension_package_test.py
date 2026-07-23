"""CLI e2e: load a SQLite extension by **package name** through the real
`dirsql` launcher + binary.

Drives the Python launcher (`dirsql.cli.main:main`) against a real
`.dirsql.toml` whose `[[dirsql.extension]].path` is a bare package name
installed on the launcher's `sys.path`, then queries the running HTTP server
and asserts the extension's function is callable. No mocks. The launcher
resolves the package name (via `importlib`) and passes the resolved literal
path to the binary as `--extension`; the compiled binary can't resolve
package names itself.

The loadable is the repo's `tests/fixtures/testext` cdylib (registers
`dirsql_testext_answer() -> 42`), built on the fly with cargo.
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
_FIXTURE_MANIFEST = os.path.join(
    _REPO_ROOT, "packages", "rust", "tests", "fixtures", "testext", "Cargo.toml"
)
_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "dirsql")
_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "dirsql")
_BINARY = _RELEASE if os.path.exists(_RELEASE) else _DEBUG

# Where the launcher's `binary_path()` looks: `<dirsql package>/_binary/dirsql`.
import dirsql as _dirsql_pkg  # noqa: E402

_BINARY_STAGE_DIR = os.path.join(os.path.dirname(_dirsql_pkg.__file__), "_binary")


def _build_fixture_extension(target_dir):
    proc = subprocess.run(
        [
            os.environ.get("CARGO", "cargo"),
            "build",
            "--manifest-path",
            _FIXTURE_MANIFEST,
            "--target-dir",
            target_dir,
            "--message-format=json",
        ],
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 0, f"fixture build failed:\n{proc.stderr}"
    artifact = None
    for line in proc.stdout.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") == "compiler-artifact":
            for f in msg.get("filenames") or []:
                if f.endswith((".so", ".dylib", ".dll")):
                    artifact = f
    assert artifact, "no cdylib artifact in cargo build output"
    return artifact


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


def _query(port, sql):
    body = json.dumps({"sql": sql}).encode()
    req = urllib.request.Request(
        f"http://localhost:{port}/query",
        data=body,
        headers={"content-type": "application/json"},
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read())


def describe_cli_extension_by_package_name():
    @pytest.fixture
    def staged(tmp_path):
        if shutil.which(os.environ.get("CARGO", "cargo")) is None:
            pytest.skip("cargo not available to build the extension fixture")
        assert os.path.exists(_BINARY), (
            f"dirsql binary not built at {_BINARY}; "
            "run `cargo build -p dirsql --features cli` first"
        )
        so = _build_fixture_extension(str(tmp_path / "target"))

        # A real installed package on the launcher's sys.path (via PYTHONPATH).
        env_dir = tmp_path / "env"
        pkg = env_dir / "dirsql_testext_pkg"
        pkg.mkdir(parents=True)
        (pkg / "__init__.py").write_text("")
        shutil.copy(so, pkg / os.path.basename(so))

        # Stage the binary where the launcher's `binary_path()` looks.
        os.makedirs(_BINARY_STAGE_DIR, exist_ok=True)
        staged_binary = os.path.join(_BINARY_STAGE_DIR, "dirsql")
        shutil.copy(_BINARY, staged_binary)
        os.chmod(staged_binary, 0o755)
        try:
            yield str(env_dir)
        finally:
            shutil.rmtree(_BINARY_STAGE_DIR, ignore_errors=True)

    def it_resolves_loads_and_queries_a_package_name_extension(staged, tmp_path):
        env_dir = staged
        root = tmp_path / "data"
        root.mkdir()
        (root / "a.txt").write_text("x")
        cfg = root / ".dirsql.toml"
        cfg.write_text(
            "[[dirsql.extension]]\n"
            'path = "dirsql_testext_pkg"\n'
            'entrypoint = "sqlite3_extension_init"\n\n'
            "[[table]]\n"
            'ddl = "CREATE TABLE files (path TEXT)"\n'
            'glob = "*.txt"\n'
        )

        port = _free_port()
        env = {
            **os.environ,
            "PYTHONPATH": env_dir + os.pathsep + os.environ.get("PYTHONPATH", ""),
        }
        proc = subprocess.Popen(
            [
                sys.executable,
                "-c",
                "import sys; from dirsql.cli.main import main; sys.exit(main())",
                "server",
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
            env=env,
        )
        try:
            ok = _wait_for_server(proc, port)
            if not ok:
                out, err = proc.communicate(timeout=5)
                raise AssertionError(
                    f"server did not start\n--- stdout ---\n{out}\n--- stderr ---\n{err}"
                )
            assert _query(port, "SELECT dirsql_testext_answer() AS a") == [{"a": 42}]
        finally:
            proc.terminate()
            proc.wait(timeout=5)
