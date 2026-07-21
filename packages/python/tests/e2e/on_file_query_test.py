"""CLI e2e: `dirsql query --on-file '<command>'` through the real launcher +
bundled binary (docs/reference/path-tables.md#parsing-rows-with-on-file).

Runs the Python launcher (`dirsql.cli.main:main`) as a subprocess over a real
temp tree with a real parser script. Asserts the documented behavior end to
end: the parser supplies the rows and schema, a failing file is isolated with a
stderr warning while the good files still return, and a repeated flag errors.
No mocks: real launcher, real binary, real process, real filesystem.
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

import dirsql as _dirsql_pkg  # noqa: E402

_BINARY_STAGE_DIR = os.path.join(os.path.dirname(_dirsql_pkg.__file__), "_binary")

_PARSER = """#!/bin/sh
f="$1"
if grep -q POISON "$f"; then echo "poison in $f" >&2; exit 7; fi
title=$(head -n1 "$f" | sed 's/^# //')
printf '[{"title":"%s"}]\\n' "$title"
"""


def describe_on_file_query():
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
        (root / "docs" / "a.md").write_text("# Alpha\nbody\n")
        (root / "docs" / "b.md").write_text("# Bravo\nbody\n")
        parser = root / "parse.sh"
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
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )

    def it_supplies_rows_and_schema_from_the_parser(tree):
        proc = _run(
            tree,
            "query",
            "SELECT title FROM './docs/*.md'",
            "--on-file",
            "./parse.sh {path}",
        )
        assert proc.returncode == 0, proc.stderr
        rows = json.loads(proc.stdout)
        titles = sorted(r["title"] for r in rows)
        assert titles == ["Alpha", "Bravo"]

    def it_isolates_a_failing_file_and_warns(tree):
        (tree / "docs" / "bad.md").write_text("POISON\n")
        proc = _run(
            tree,
            "query",
            "SELECT title FROM './docs/*.md'",
            "--on-file",
            "./parse.sh {path}",
        )
        assert proc.returncode == 0, proc.stderr
        titles = sorted(r["title"] for r in json.loads(proc.stdout))
        assert titles == ["Alpha", "Bravo"]
        assert "bad.md" in proc.stderr

    def it_rejects_a_repeated_flag_pointing_at_config_files(tree):
        proc = _run(
            tree,
            "query",
            "SELECT title FROM './docs/*.md'",
            "--on-file",
            "./parse.sh {path}",
            "--on-file",
            "cat {path}",
        )
        assert proc.returncode != 0
        assert "config file" in proc.stderr
