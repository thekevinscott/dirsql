"""CLI e2e: a glob `{name}` placeholder colliding with a declared DDL column
is a load-time error through the real launcher + binary.

Captures no longer populate columns, so a placeholder whose name is also a
declared column would read NULL forever. The launcher must exit non-zero and
surface the collision diagnostic. No mocks: real launcher, real binary, real
process, real filesystem.
"""

import os
import shutil
import subprocess
import sys

import dirsql as _dirsql_pkg

_REPO_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..")
)
_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "dirsql")
_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "dirsql")
_BINARY = _RELEASE if os.path.exists(_RELEASE) else _DEBUG

_BINARY_STAGE_DIR = os.path.join(os.path.dirname(_dirsql_pkg.__file__), "_binary")


def describe_capture_column_collision():
    def it_exits_nonzero_and_names_the_collision(tmp_path):
        assert os.path.exists(_BINARY), (
            f"dirsql binary not built at {_BINARY}; "
            "run `cargo build -p dirsql --features cli` first"
        )
        os.makedirs(_BINARY_STAGE_DIR, exist_ok=True)
        staged_binary = os.path.join(_BINARY_STAGE_DIR, "dirsql")
        shutil.copy(_BINARY, staged_binary)
        os.chmod(staged_binary, 0o755)

        try:
            root = tmp_path / "data"
            (root / "_comments" / "abc123").mkdir(parents=True)
            (root / "_comments" / "abc123" / "first.txt").write_text("hi")
            cfg = root / ".dirsql.toml"
            cfg.write_text(
                "[[table]]\n"
                'name = "comments"\n'
                'ddl = "CREATE TABLE comments (thread_id TEXT, basename TEXT)"\n'
                'glob = "_comments/{thread_id}/*.txt"\n'
                'on-file = "cat {path}"\n'
            )

            proc = subprocess.run(
                [
                    sys.executable,
                    "-c",
                    "import sys; from dirsql.cli.main import main; sys.exit(main())",
                    "query",
                    "SELECT * FROM comments",
                    "--config",
                    str(cfg),
                ],
                cwd=str(root),
                capture_output=True,
                text=True,
                timeout=30,
            )

            assert proc.returncode != 0, (
                f"a colliding capture config must exit non-zero, got {proc!r}"
            )
            assert "thread_id" in proc.stderr and "collides" in proc.stderr, (
                f"stderr must name the colliding placeholder/column, got {proc.stderr!r}"
            )
        finally:
            shutil.rmtree(_BINARY_STAGE_DIR, ignore_errors=True)
