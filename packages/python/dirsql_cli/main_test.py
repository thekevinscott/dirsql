"""Unit tests for the launcher's `main`."""

import io
import os
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
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(main_module, "with_resolved_extensions", lambda a: a),
                patch(
                    "dirsql_cli.main.subprocess.run", return_value=_Completed(7)
                ) as run,
            ):
                assert main(["--version"]) == 7
            run.assert_called_once_with(["C:/dirsql.exe", "--version"])

        def it_does_not_invoke_execv():
            with (
                patch.object(main_module, "binary_path", return_value="C:/dirsql.exe"),
                patch.object(main_module, "is_windows", return_value=True),
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(main_module, "with_resolved_extensions", lambda a: a),
                patch("dirsql_cli.main.subprocess.run", return_value=_Completed(0)),
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
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(main_module, "with_resolved_extensions", lambda a: a),
                patch.object(os, "execv") as execv,
            ):
                main(["query", "select 1"])
            execv.assert_called_once_with(
                "/usr/local/bin/dirsql",
                ["/usr/local/bin/dirsql", "query", "select 1"],
            )

        def it_resolves_config_extensions_before_handing_off():
            with (
                patch.object(main_module, "binary_path", return_value="/bin/dirsql"),
                patch.object(main_module, "is_windows", return_value=False),
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(
                    main_module,
                    "with_resolved_extensions",
                    lambda a: [*a, "--extension", "/abs/x.so"],
                ),
                patch.object(os, "execv") as execv,
            ):
                main(["--config", "cfg.toml"])
            execv.assert_called_once_with(
                "/bin/dirsql",
                ["/bin/dirsql", "--config", "cfg.toml", "--extension", "/abs/x.so"],
            )

        def it_discovers_plugins_before_resolving_extensions():
            # Discovery injects `-c <fragment>` into argv; extension resolution
            # must then see the already-injected argv (so a fragment's own
            # extensions could be resolved). Order matters.
            seen: list[list[str]] = []
            with (
                patch.object(main_module, "binary_path", return_value="/bin/dirsql"),
                patch.object(main_module, "is_windows", return_value=False),
                patch.object(
                    main_module,
                    "with_discovered_plugins",
                    lambda a: [*a, "-c", "/frag.toml"],
                ),
                patch.object(
                    main_module,
                    "with_resolved_extensions",
                    lambda a: seen.append(list(a)) or a,
                ),
                patch.object(os, "execv"),
            ):
                main([])
            assert seen == [["-c", "/frag.toml"]]

        def it_returns_1_when_extension_resolution_fails():
            fake_stderr = io.StringIO()
            with (
                patch.object(main_module, "binary_path", return_value="/bin/dirsql"),
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(
                    main_module,
                    "with_resolved_extensions",
                    side_effect=ValueError("not installed"),
                ),
                patch.object(sys, "stderr", fake_stderr),
            ):
                assert main(["--config", "cfg.toml"]) == 1
            assert "not installed" in fake_stderr.getvalue()

        def it_returns_0_when_execv_returns():
            # In production `os.execv` never returns. The mock returns,
            # so `main` falls through to `return 0`.
            with (
                patch.object(main_module, "binary_path", return_value="/bin/dirsql"),
                patch.object(main_module, "is_windows", return_value=False),
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(main_module, "with_resolved_extensions", lambda a: a),
                patch.object(os, "execv"),
            ):
                assert main([]) == 0

    def describe_when_argv_is_none():
        def it_pulls_from_sys_argv_skipping_the_program_name():
            with (
                patch.object(sys, "argv", ["dirsql", "--help"]),
                patch.object(main_module, "binary_path", return_value="C:/dirsql.exe"),
                patch.object(main_module, "is_windows", return_value=True),
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(main_module, "with_resolved_extensions", lambda a: a),
                patch(
                    "dirsql_cli.main.subprocess.run", return_value=_Completed(0)
                ) as run,
            ):
                main(argv=None)
            run.assert_called_once_with(["C:/dirsql.exe", "--help"])

    def describe_when_argv0_is_interpret():
        """`interpret` is forwarded to the bundled Rust binary like any other
        argv (the binary rejects it as an unknown subcommand)."""

        def it_forwards_interpret_to_the_binary_instead_of_intercepting():
            with (
                patch.object(main_module, "binary_path", return_value="/bin/dirsql"),
                patch.object(main_module, "is_windows", return_value=True),
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(main_module, "with_resolved_extensions", lambda a: a),
                patch(
                    "dirsql_cli.main.subprocess.run", return_value=_Completed(2)
                ) as run,
            ):
                assert main(["interpret", "config.py"]) == 2
            run.assert_called_once_with(["/bin/dirsql", "interpret", "config.py"])
