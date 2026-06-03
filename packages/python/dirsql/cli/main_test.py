"""Unit tests for the launcher's `main`."""

import io
import sys
from unittest.mock import patch

from dirsql.cli.main import main


class _Completed:
    def __init__(self, returncode: int):
        self.returncode = returncode


def describe_main():
    def describe_when_binary_path_raises_file_not_found():
        def it_returns_1():
            def raises():
                raise FileNotFoundError("bundled `dirsql` missing")

            code = main(
                argv=[],
                binary_path_fn=raises,
                stderr=io.StringIO(),
            )
            assert code == 1

        def it_writes_a_dirsql_prefixed_message_to_stderr():
            def raises():
                raise FileNotFoundError("bundled `dirsql` missing")

            stderr = io.StringIO()
            main(
                argv=[],
                binary_path_fn=raises,
                stderr=stderr,
            )
            assert stderr.getvalue() == "dirsql: bundled `dirsql` missing\n"

    def describe_on_windows():
        def it_runs_the_binary_via_subprocess_and_returns_its_returncode():
            calls = []

            def fake_run(cmd):
                calls.append(cmd)
                return _Completed(7)

            code = main(
                argv=["--version"],
                binary_path_fn=lambda: "C:/dirsql.exe",
                is_windows_fn=lambda: True,
                subprocess_run=fake_run,
            )

            assert code == 7
            assert calls == [["C:/dirsql.exe", "--version"]]

        def it_does_not_invoke_execv():
            execv_calls = []

            main(
                argv=[],
                binary_path_fn=lambda: "C:/dirsql.exe",
                is_windows_fn=lambda: True,
                subprocess_run=lambda _cmd: _Completed(0),
                execv=lambda *a, **kw: execv_calls.append((a, kw)),
            )
            assert execv_calls == []

    def describe_on_posix():
        def it_hands_off_to_execv_with_the_binary_argv_pair():
            execv_calls = []

            main(
                argv=["query", "select 1"],
                binary_path_fn=lambda: "/usr/local/bin/dirsql",
                is_windows_fn=lambda: False,
                execv=lambda path, args: execv_calls.append((path, args)),
            )

            assert execv_calls == [
                ("/usr/local/bin/dirsql", ["/usr/local/bin/dirsql", "query", "select 1"])
            ]

        def it_returns_0_when_execv_returns():
            # In production `os.execv` never returns. In tests the fake
            # returns, and `main` falls through to `return 0`.
            code = main(
                argv=[],
                binary_path_fn=lambda: "/bin/dirsql",
                is_windows_fn=lambda: False,
                execv=lambda _p, _a: None,
            )
            assert code == 0

    def describe_when_argv_is_none():
        def it_pulls_from_sys_argv_skipping_the_program_name():
            captured = []

            with patch.object(sys, "argv", ["dirsql", "--help"]):
                main(
                    argv=None,
                    binary_path_fn=lambda: "C:/dirsql.exe",
                    is_windows_fn=lambda: True,
                    subprocess_run=lambda cmd: (
                        captured.append(cmd),
                        _Completed(0),
                    )[1],
                )

            assert captured == [["C:/dirsql.exe", "--help"]]
