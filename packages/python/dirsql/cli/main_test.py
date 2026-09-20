"""Unit tests for the launcher's `main`."""

import io
import sys
from unittest.mock import patch

from . import main as main_module
from .main import keep_sigint_fatal, main


def describe_keep_sigint_fatal():
    def it_installs_the_default_disposition_and_returns_the_previous_handler():
        recorded = {}

        def fake_signal(signum, new):
            recorded["signum"] = signum
            recorded["new"] = new
            return "prior-handler"

        assert keep_sigint_fatal(fake_signal) == "prior-handler"
        assert recorded["signum"] == main_module.signal.SIGINT
        assert recorded["new"] is main_module.signal.SIG_DFL


def describe_main():
    def describe_when_argv_middleware_raises():
        def it_returns_1_and_writes_a_dirsql_prefixed_message_to_stderr():
            fake_stderr = io.StringIO()
            with (
                patch.object(
                    main_module,
                    "with_discovered_plugins",
                    side_effect=RuntimeError("plugin blew up"),
                ),
                patch.object(sys, "stderr", fake_stderr),
            ):
                assert main([]) == 1
            assert fake_stderr.getvalue() == "dirsql: plugin blew up\n"

    def describe_when_the_core_runs():
        def it_returns_the_cores_exit_code():
            with (
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(main_module, "with_resolved_extensions", lambda a: a),
                patch.object(main_module, "keep_sigint_fatal", return_value="prior"),
                patch.object(main_module.signal, "signal"),
                patch.object(
                    main_module, "run_in_process", return_value=23
                ) as run_in_process,
            ):
                assert main(["query", "SELECT 1"]) == 23
            run_in_process.assert_called_once_with(
                argv=["query", "SELECT 1"], module="dirsql._dirsql"
            )

        def it_discovers_plugins_before_resolving_extensions():
            # Discovery injects `-c <fragment>`; extension resolution must see
            # that fragment, so the order is load-bearing.
            with (
                patch.object(
                    main_module, "with_discovered_plugins", lambda a: [*a, "-c", "p"]
                ),
                patch.object(
                    main_module,
                    "with_resolved_extensions",
                    lambda a: [*a, "--extension", "/r/vec0"],
                ),
                patch.object(main_module, "keep_sigint_fatal", return_value="prior"),
                patch.object(main_module.signal, "signal"),
                patch.object(
                    main_module, "run_in_process", return_value=0
                ) as run_in_process,
            ):
                assert main(["query"]) == 0
            assert run_in_process.call_args.kwargs["argv"] == [
                "query",
                "-c",
                "p",
                "--extension",
                "/r/vec0",
            ]

        def it_defaults_argv_to_sys_argv_minus_the_program_name():
            with (
                patch.object(sys, "argv", ["dirsql", "--version"]),
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(main_module, "with_resolved_extensions", lambda a: a),
                patch.object(main_module, "keep_sigint_fatal", return_value="prior"),
                patch.object(main_module.signal, "signal"),
                patch.object(
                    main_module, "run_in_process", return_value=0
                ) as run_in_process,
            ):
                assert main() == 0
            assert run_in_process.call_args.kwargs["argv"] == ["--version"]

    def describe_when_the_core_cannot_be_reached():
        def it_returns_1_with_the_message():
            fake_stderr = io.StringIO()
            with (
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(main_module, "with_resolved_extensions", lambda a: a),
                patch.object(main_module, "keep_sigint_fatal", return_value="prior"),
                patch.object(main_module.signal, "signal"),
                patch.object(
                    main_module,
                    "run_in_process",
                    side_effect=RuntimeError("no run_cli export"),
                ),
                patch.object(sys, "stderr", fake_stderr),
            ):
                assert main([]) == 1
            assert fake_stderr.getvalue() == "dirsql: no run_cli export\n"

    def describe_signal_ownership():
        def it_restores_the_previous_handler_after_the_run():
            restored = []
            with (
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(main_module, "with_resolved_extensions", lambda a: a),
                patch.object(main_module, "keep_sigint_fatal", return_value="prior"),
                patch.object(
                    main_module.signal,
                    "signal",
                    lambda signum, h: restored.append((signum, h)),
                ),
                patch.object(main_module, "run_in_process", return_value=0),
            ):
                assert main([]) == 0
            assert restored == [(main_module.signal.SIGINT, "prior")]

        def it_restores_the_previous_handler_even_when_the_run_raises():
            restored = []
            with (
                patch.object(main_module, "with_discovered_plugins", lambda a: a),
                patch.object(main_module, "with_resolved_extensions", lambda a: a),
                patch.object(main_module, "keep_sigint_fatal", return_value="prior"),
                patch.object(
                    main_module.signal,
                    "signal",
                    lambda signum, h: restored.append((signum, h)),
                ),
                patch.object(
                    main_module, "run_in_process", side_effect=RuntimeError("boom")
                ),
                patch.object(sys, "stderr", io.StringIO()),
            ):
                assert main([]) == 1
            assert restored == [(main_module.signal.SIGINT, "prior")]
