"""Integration tests for the `dirsql interpret` entry point.

In-process: drives the real `run()` against real config modules (real
`load_app` + real `DirSQL`/core). stdout/stderr are read via pytest's `capsys`
fixture (patching `sys.std*` in a fixture would lose to pytest's own call-phase
capture); stdin is pytest's empty captured stdin. The cheaper, CI-running
mirror of the subprocess e2e tests in `tests/e2e/interpret_test.py`. Config
fixtures are inlined as module-level strings so the test owns its data.
"""

from __future__ import annotations

import asyncio
import contextlib
import io
import json
import os
import sys
from unittest.mock import patch

import pytest

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


@pytest.fixture
def interpret_env():
    """Mirror a fresh `dirsql interpret` process: a current event loop (DirSQL
    construction schedules a background init task on it; the loop never runs --
    interpret is synchronous -- so the task stays pending, as in production) and
    an empty stdin so the read loop exits right after the handshake. stdout and
    stderr are read via the `capsys` fixture instead -- patching them here would
    lose to pytest's own call-phase capture, but the stdin patch survives."""
    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    with patch.object(sys, "stdin", io.StringIO("")):
        try:
            yield
        finally:
            asyncio.set_event_loop(None)
            loop.close()


def describe_interpret_integration():
    def it_defaults_root_to_cwd_when_the_config_omits_root(
        tmp_path, capsys, interpret_env
    ):
        """A config with neither `root` nor `config` resolves the handshake
        root to the process cwd."""
        cfg = tmp_path / "config.py"
        cfg.write_text(NO_ROOT_CONFIG)
        cwd = os.path.realpath(tmp_path)
        with _chdir(cwd):
            rc = run([str(cfg)])
        assert rc == 0, "expected interpret to start with a root-less config"
        handshake = json.loads(capsys.readouterr().out.splitlines()[0])
        assert handshake["state"]["root"] == cwd

    def it_rejects_a_config_that_sets_config(tmp_path, capsys, interpret_env):
        """A config whose `app` itself sets `config=` is rejected: nested
        config loading is unsupported."""
        (tmp_path / "nested.dirsql.toml").write_text(NESTED_TOML)
        cfg = tmp_path / "config.py"
        cfg.write_text(NESTED_CONFIG)
        rc = run([str(cfg)])
        assert rc != 0, "expected interpret to reject a nested config"
        assert "config" in capsys.readouterr().err.lower()
