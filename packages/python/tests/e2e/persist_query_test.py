"""CLI e2e: `dirsql query ... --persist` over a parsed path-table
(docs/howto/persist.md).

Runs the Python launcher (`dirsql.cli.main:main`) as a subprocess twice over the
same unchanged temp tree, with a real parser script that records every
invocation. The second run must serve the rows from the cache: the parser runs
for no file and the cache file is not rewritten. No mocks: real launcher, real
binary, real process, real filesystem.
"""

import json
import os
import shutil
import subprocess
import sys

_REPO_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..")
)
_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "dirsql")
_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "dirsql")
_BINARY = _RELEASE if os.path.exists(_RELEASE) else _DEBUG

import dirsql as _dirsql_pkg

_BINARY_STAGE_DIR = os.path.join(os.path.dirname(_dirsql_pkg.__file__), "_binary")

_PARSER = """#!/bin/sh
printf x >> "$2"
cat "$1"
"""


def describe_persist_query():
    import pytest

    @pytest.fixture
    def tree(tmp_path):
        assert os.path.exists(_BINARY), (
            f"dirsql binary not built at {_BINARY}; "
            "run `cargo build -p dirsql --features cli` first"
        )
        os.makedirs(_BINARY_STAGE_DIR, exist_ok=True)
        staged = os.path.join(_BINARY_STAGE_DIR, "dirsql")
        shutil.copy(_BINARY, staged)
        os.chmod(staged, 0o755)

        root = tmp_path / "data"
        (root / "docs").mkdir(parents=True)
        for i in range(5):
            (root / "docs" / f"a{i}.json").write_text(
                json.dumps([{"id": i, "tag": "v1"}])
            )
        parser = tmp_path / "parse.sh"
        parser.write_text(_PARSER)
        os.chmod(parser, 0o755)
        try:
            yield root
        finally:
            shutil.rmtree(_BINARY_STAGE_DIR, ignore_errors=True)

    def _run(root, *args):
        return subprocess.run(
            [
                sys.executable,
                "-c",
                "import sys; from dirsql.cli.main import main; sys.exit(main())",
                *args,
            ],
            cwd=str(root),
            capture_output=True,
            text=True,
        )

    def _query(root, counter):
        return _run(
            root,
            "query",
            "SELECT id, tag FROM './docs/*.json'",
            "--on-file",
            f"sh {root.parent / 'parse.sh'} {{path}} {counter}",
            "--persist",
        )

    def _parses(counter):
        return os.path.getsize(counter) if os.path.exists(counter) else 0

    def it_serves_the_second_run_from_the_cache(tree):
        counter = tree.parent / "parses"

        cold = _query(tree, counter)
        assert cold.returncode == 0, cold.stderr
        assert _parses(counter) == 5, "the cold run parses every file once"
        cold_rows = sorted(json.loads(cold.stdout), key=lambda r: r["id"])

        cache = tree / ".dirsql" / "cache.db"
        before = cache.read_bytes()
        counter.write_bytes(b"")

        warm = _query(tree, counter)
        assert warm.returncode == 0, warm.stderr
        assert _parses(counter) == 0, "an unchanged tree must not re-run the parser"
        assert sorted(json.loads(warm.stdout), key=lambda r: r["id"]) == cold_rows
        assert cache.read_bytes() == before, (
            "an unchanged tree must not rewrite the cache"
        )

    def it_reparses_only_a_changed_file(tree):
        counter = tree.parent / "parses"
        assert _query(tree, counter).returncode == 0
        counter.write_bytes(b"")

        changed = tree / "docs" / "a3.json"
        changed.write_text(json.dumps([{"id": 3, "tag": "v2"}]))
        future = os.path.getmtime(changed) + 5
        os.utime(changed, (future, future))

        warm = _query(tree, counter)
        assert warm.returncode == 0, warm.stderr
        assert _parses(counter) == 1, "only the changed file is re-parsed"
        rows = {r["id"]: r["tag"] for r in json.loads(warm.stdout)}
        assert rows == {0: "v1", 1: "v1", 2: "v1", 3: "v2", 4: "v1"}
