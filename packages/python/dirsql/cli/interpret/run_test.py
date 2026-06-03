"""Unit tests for `run`."""

import io
import json
import sys
from types import SimpleNamespace
from unittest.mock import patch

from . import run as run_module
from .run import run


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
