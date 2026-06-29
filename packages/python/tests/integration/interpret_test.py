"""Integration tests for the `dirsql interpret` entry point.

In-process: drives `run()` directly with a real `DirSQL`/core, mocking only
the I/O boundaries (`sys.stdin` / `sys.stdout` / `sys.stderr`). This is the
cheaper, CI-running mirror of the subprocess e2e tests in
`tests/e2e/interpret_test.py`. Config fixtures are inlined as module-level
strings so the test owns its data.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import sys
from unittest.mock import patch

from dirsql.cli.interpret.run import run

NO_ROOT_CONFIG = """\
import json

from dirsql import DirSQL, Table


def _extract(path):
    with open(path, encoding="utf-8") as f:
        return [json.load(f)]


app = DirSQL(
    tables=[
        Table(
            ddl="CREATE TABLE papers (title TEXT)",
            glob="**/meta.json",
            extract=_extract,
        )
    ],
)
"""

NESTED_CONFIG = """\
import os

from dirsql import DirSQL

app = DirSQL(config=os.path.join(os.path.dirname(__file__), "nested.dirsql.toml"))
"""

NESTED_TOML = (
    '[[table]]\nddl = "CREATE TABLE papers (title TEXT)"\nglob = "**/meta.json"\n'
)


@contextlib.contextmanager
def _chdir(path):
    prev = os.getcwd()
    os.chdir(path)
    try:
        yield
    finally:
        os.chdir(prev)


def describe_interpret_integration():
    def it_defaults_root_to_cwd_when_the_config_omits_root(tmp_path):
        """A config with neither `root` nor `config` resolves the handshake
        root to the process cwd."""
        cfg = tmp_path / "config.py"
        cfg.write_text(NO_ROOT_CONFIG)
        cwd = os.path.realpath(tmp_path)
        out = io.StringIO()
        with (
            _chdir(cwd),
            patch.object(sys, "stdin", io.StringIO("")),
            patch.object(sys, "stdout", out),
            patch.object(sys, "stderr", io.StringIO()),
        ):
            rc = run([str(cfg)])
        assert rc == 0, "expected interpret to start with a root-less config"
        handshake = json.loads(out.getvalue().splitlines()[0])
        assert handshake["state"]["root"] == cwd

    def it_rejects_a_config_that_sets_config(tmp_path):
        """A config whose `app` itself sets `config=` is rejected: nested
        config loading is unsupported."""
        (tmp_path / "nested.dirsql.toml").write_text(NESTED_TOML)
        cfg = tmp_path / "config.py"
        cfg.write_text(NESTED_CONFIG)
        err = io.StringIO()
        with (
            patch.object(sys, "stdin", io.StringIO("")),
            patch.object(sys, "stdout", io.StringIO()),
            patch.object(sys, "stderr", err),
        ):
            rc = run([str(cfg)])
        assert rc != 0, "expected interpret to reject a nested config"
        assert "config" in err.getvalue().lower()
