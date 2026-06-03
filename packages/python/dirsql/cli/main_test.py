"""Unit tests for the launcher's `main`."""

import io
import os
import subprocess
import sys
from unittest.mock import patch

from . import main as main_module
from .main import main


class _Completed:
    def __init__(self, returncode: int):
        self.returncode = returncode


def describe_main():
    def describe_when_binary_path_raises_file_not_found():
        def it_returns_1_and_writes_a_dirsql_prefixed_message_to_stderr():
            fake_stderr = io.StringIO()
            with (
                patch.object(
                    main_module,
                    "binary_path",
                    side_effect=FileNotFoundError("bundled `dirsql` missing"),
                ),
                patch.object(sys, "stderr", fake_stderr),
            ):
                assert main([]) == 1
            assert fake_stderr.getvalue() == "dirsql: bundled `dirsql` missing\n"

    def describe_on_windows():
        def it_runs_the_binary_via_subprocess_and_returns_its_returncode():
            with (
                patch.object(main_module, "binary_path", return_value="C:/dirsql.exe"),
                patch.object(main_module, "is_windows", return_value=True),
                patch.object(subprocess, "run", return_value=_Completed(7)) as run,
            ):
                assert main(["--version"]) == 7
            run.assert_called_once_with(["C:/dirsql.exe", "--version"])

        def it_does_not_invoke_execv():
            with (
                patch.object(main_module, "binary_path", return_value="C:/dirsql.exe"),
                patch.object(main_module, "is_windows", return_value=True),
                patch.object(subprocess, "run", return_value=_Completed(0)),
                patch.object(os, "execv") as execv,
            ):
                main([])
            execv.assert_not_called()

    def describe_on_posix():
        def it_hands_off_to_execv_with_the_binary_argv_pair():
            with (
                patch.object(
                    main_module, "binary_path", return_value="/usr/local/bin/dirsql"
                ),
                patch.object(main_module, "is_windows", return_value=False),
                patch.object(os, "execv") as execv,
            ):
                main(["query", "select 1"])
            execv.assert_called_once_with(
                "/usr/local/bin/dirsql",
                ["/usr/local/bin/dirsql", "query", "select 1"],
            )

        def it_returns_0_when_execv_returns():
            # In production `os.execv` never returns. The mock returns,
            # so `main` falls through to `return 0`.
            with (
                patch.object(main_module, "binary_path", return_value="/bin/dirsql"),
                patch.object(main_module, "is_windows", return_value=False),
                patch.object(os, "execv"),
            ):
                assert main([]) == 0

    def describe_when_argv_is_none():
        def it_pulls_from_sys_argv_skipping_the_program_name():
            with (
                patch.object(sys, "argv", ["dirsql", "--help"]),
                patch.object(main_module, "binary_path", return_value="C:/dirsql.exe"),
                patch.object(main_module, "is_windows", return_value=True),
                patch.object(subprocess, "run", return_value=_Completed(0)) as run,
            ):
                main(argv=None)
            run.assert_called_once_with(["C:/dirsql.exe", "--help"])

    def describe_when_argv0_is_interpret():
        """`dirsql interpret <config>` is handled in-process so the Rust
        orchestrator can spawn the launcher for native-language configs
        (#196) without depending on the bundled Rust binary."""

        def it_dispatches_to_interpret_run_and_returns_its_exit_code():
            # main.py does `from .interpret import run` lazily, so
            # patching the package's `run` attribute is sufficient --
            # each call resolves the name freshly from the package.
            from . import interpret as interpret_pkg

            with (
                patch.object(interpret_pkg, "run", return_value=0) as interpret_run,
                patch.object(main_module, "binary_path") as binary_path,
            ):
                assert main(["interpret", "config.py"]) == 0
            interpret_run.assert_called_once_with(["config.py"])
            binary_path.assert_not_called()

        def it_propagates_a_nonzero_interpret_exit():
            from . import interpret as interpret_pkg

            with patch.object(interpret_pkg, "run", return_value=2):
                assert main(["interpret", "bad.py"]) == 2

        def it_returns_130_on_keyboard_interrupt():
            from . import interpret as interpret_pkg

            with patch.object(interpret_pkg, "run", side_effect=KeyboardInterrupt()):
                assert main(["interpret", "config.py"]) == 130

        def it_does_not_intercept_when_interpret_is_not_argv_0():
            # `dirsql --verbose interpret` is the binary's problem, not
            # ours -- the in-process route only fires on the first arg.
            with (
                patch.object(main_module, "binary_path", return_value="/bin/dirsql"),
                patch.object(main_module, "is_windows", return_value=True),
                patch.object(subprocess, "run", return_value=_Completed(0)),
            ):
                main(["--verbose", "interpret", "config.py"])
            # binary_path was reached, meaning we did NOT take the
            # interpret shortcut.
