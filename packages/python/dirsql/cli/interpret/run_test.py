"""Unit tests for `run`."""

import io
import json
import os
import sys
from types import SimpleNamespace
from unittest.mock import patch

import pytest

from . import run as run_module
from .run import run

# Literal copy of `dirsql.resolve_config.INTERPRET_ROOT_ENV`. Inlined rather
# than imported so this unit test stays isolated from that collaborator
# module (testing-conventions `unit lint`); the integration tests exercise
# the two sides agreeing on the name.
INTERPRET_ROOT_ENV = "DIRSQL_INTERPRET_ROOT"


@pytest.fixture(autouse=True)
def _isolate_environ():
    """`run` writes DIRSQL_INTERPRET_ROOT into ``os.environ`` (#251). Scope
    that mutation to a per-test copy so it never leaks into the process
    environment other tests observe."""
    with patch.object(run_module.os, "environ", dict(os.environ)):
        yield


def _fake_table(name: str, extract):
    return SimpleNamespace(name=name, extract=extract)


def _fake_app(tables: list, state: dict):
    """Build a duck-typed `app` -- `_tables` for dispatch, `__dict__`
    handled by overriding `vars`."""

    class _App:
        _tables = tables

    a = _App()
    # vars(a) needs to return `state` -- bind it through __dict__.
    a.__dict__.update(state)
    return a


def describe_run():
    def describe_argv_validation():
        def it_returns_1_when_argv_has_no_config_path():
            fake_stderr = io.StringIO()
            with patch.object(sys, "stderr", fake_stderr):
                assert run([]) == 1
            assert fake_stderr.getvalue().startswith("dirsql interpret:")
            assert "got 0" in fake_stderr.getvalue()

        def it_returns_1_when_argv_has_multiple_paths():
            fake_stderr = io.StringIO()
            with patch.object(sys, "stderr", fake_stderr):
                assert run(["a.py", "b.py"]) == 1
            assert "got 2" in fake_stderr.getvalue()

    def describe_load_failure():
        def it_returns_1_and_writes_a_dirsql_prefixed_line_to_stderr():
            fake_stderr = io.StringIO()
            with (
                patch.object(
                    run_module,
                    "load_app",
                    side_effect=AttributeError("missing app"),
                ),
                patch.object(sys, "stderr", fake_stderr),
            ):
                assert run(["bad.py"]) == 1
            assert fake_stderr.getvalue() == "dirsql interpret: missing app\n"

    def describe_interpret_root_env():
        def it_sets_the_interpret_root_env_to_the_config_parent_before_load():
            # #251: the launcher must publish DIRSQL_INTERPRET_ROOT (the
            # config file's parent directory) so a no-root native config
            # resolves its scan root. It must be set *before* load_app, so
            # capture the env value seen by a fake load_app.
            app = _fake_app(tables=[], state={})
            fake_env: dict = {}
            seen: dict = {}

            def fake_load_app(_path):
                seen["root"] = fake_env.get(INTERPRET_ROOT_ENV)
                return app

            with (
                patch.object(run_module.os, "environ", fake_env),
                patch.object(run_module, "load_app", side_effect=fake_load_app),
                patch.object(run_module, "write_message"),
                patch.object(sys, "stdin", io.StringIO("")),
            ):
                assert run([os.path.join("some", "dir", "cfg.py")]) == 0

            expected = os.path.dirname(
                os.path.abspath(os.path.join("some", "dir", "cfg.py"))
            )
            assert seen["root"] == expected
            assert fake_env[INTERPRET_ROOT_ENV] == expected

    def describe_handshake():
        def it_writes_a_config_message_with_vars_app_as_state():
            app = _fake_app(tables=[], state={"root": "/x", "ignore": []})
            fake_stdin = io.StringIO("")  # immediate EOF
            written: list[dict] = []
            with (
                patch.object(run_module, "load_app", return_value=app),
                patch.object(run_module, "write_message", side_effect=written.append),
                patch.object(sys, "stdin", fake_stdin),
            ):
                assert run(["good.py"]) == 0
            assert written == [
                {"type": "config", "state": {"root": "/x", "ignore": []}}
            ]

    def describe_extract_loop():
        def it_dispatches_one_extract_request_and_writes_the_response():
            def extract(p):
                return [{"row": p}]

            app = _fake_app([_fake_table("papers", extract)], state={"x": 1})
            req = {"type": "extract", "id": 1, "table": "papers", "path": "/a"}
            fake_stdin = io.StringIO(json.dumps(req) + "\n")
            written: list[dict] = []
            with (
                patch.object(run_module, "load_app", return_value=app),
                patch.object(run_module, "write_message", side_effect=written.append),
                patch.object(sys, "stdin", fake_stdin),
            ):
                assert run(["good.py"]) == 0
            # written[0] is the handshake; written[1] is the response.
            assert written[1] == {
                "type": "result",
                "id": 1,
                "ok": True,
                "rows": [{"row": "/a"}],
            }

        def it_skips_blank_lines_silently():
            app = _fake_app([], state={})
            fake_stdin = io.StringIO("\n\n   \n")
            written: list[dict] = []
            with (
                patch.object(run_module, "load_app", return_value=app),
                patch.object(run_module, "write_message", side_effect=written.append),
                patch.object(sys, "stdin", fake_stdin),
            ):
                assert run(["good.py"]) == 0
            assert written == [{"type": "config", "state": {}}]  # only handshake

        def it_skips_malformed_json_silently():
            app = _fake_app([], state={})
            fake_stdin = io.StringIO("not json\n{also not\n")
            written: list[dict] = []
            with (
                patch.object(run_module, "load_app", return_value=app),
                patch.object(run_module, "write_message", side_effect=written.append),
                patch.object(sys, "stdin", fake_stdin),
            ):
                assert run(["good.py"]) == 0
            assert written == [{"type": "config", "state": {}}]

        def it_skips_non_extract_messages_silently():
            app = _fake_app([], state={})
            fake_stdin = io.StringIO('{"type": "ping"}\n')
            written: list[dict] = []
            with (
                patch.object(run_module, "load_app", return_value=app),
                patch.object(run_module, "write_message", side_effect=written.append),
                patch.object(sys, "stdin", fake_stdin),
            ):
                assert run(["good.py"]) == 0
            assert written == [{"type": "config", "state": {}}]

        def it_skips_non_dict_json_silently():
            app = _fake_app([], state={})
            fake_stdin = io.StringIO("42\n[]\n")
            written: list[dict] = []
            with (
                patch.object(run_module, "load_app", return_value=app),
                patch.object(run_module, "write_message", side_effect=written.append),
                patch.object(sys, "stdin", fake_stdin),
            ):
                assert run(["good.py"]) == 0
            assert written == [{"type": "config", "state": {}}]

        def it_treats_a_none_tables_attribute_as_empty():
            app = _fake_app(tables=None, state={})  # type: ignore[arg-type]
            fake_stdin = io.StringIO(
                '{"type":"extract","id":1,"table":"x","path":"/y"}\n'
            )
            written: list[dict] = []
            with (
                patch.object(run_module, "load_app", return_value=app),
                patch.object(run_module, "write_message", side_effect=written.append),
                patch.object(sys, "stdin", fake_stdin),
            ):
                assert run(["good.py"]) == 0
            # Handshake + unknown-table response.
            assert written[1]["ok"] is False
            assert "x" in written[1]["error"]

    def describe_exit():
        def it_returns_0_when_stdin_closes_after_a_run():
            app = _fake_app([_fake_table("t", lambda _p: [])], state={})
            fake_stdin = io.StringIO(
                '{"type":"extract","id":1,"table":"t","path":"/a"}\n'
            )
            with (
                patch.object(run_module, "load_app", return_value=app),
                patch.object(run_module, "write_message"),
                patch.object(sys, "stdin", fake_stdin),
            ):
                assert run(["good.py"]) == 0
