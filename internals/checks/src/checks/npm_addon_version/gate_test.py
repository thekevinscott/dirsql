import os
from unittest import mock

import pytest

from checks.npm_addon_version.gate import (
    PRINT_VERSION,
    ProbeError,
    addon_names,
    committed_version,
    list_names,
    require_zero,
    run,
)

ADDON = "dirsql.linux-x64-gnu.node"


def _result(returncode=0, stdout="", stderr=""):
    return mock.Mock(returncode=returncode, stdout=stdout, stderr=stderr)


def _run_with(runner, literal="0.2.7"):
    return run(
        "dist",
        "Cargo.toml",
        runner=runner,
        listdir=mock.Mock(return_value=[ADDON]),
        committed=mock.Mock(return_value=literal),
    )


def describe_collaborators():
    def it_resolves_the_probe_error_from_the_shared_probe_package():
        assert ProbeError.__module__ == "checks.probe.probe_error"

    def it_resolves_require_zero_from_the_shared_probe_package():
        assert require_zero.__module__ == "checks.probe.require_zero"

    def it_resolves_list_names_from_the_wheel_probe():
        assert list_names.__module__ == "checks.wheel_extension_load.list_names"

    def it_resolves_addon_names_from_its_own_module():
        assert addon_names.__module__ == "checks.npm_addon_version.addon_names"

    def it_defaults_to_reading_the_committed_version():
        assert run.__defaults__[-1] is committed_version


def describe_run():
    def skips_cleanly_when_no_addon(capsys):
        runner = mock.Mock()
        listdir = mock.Mock(return_value=["package.json"])
        assert run("dist", "Cargo.toml", runner=runner, listdir=listdir) == 0
        runner.assert_not_called()
        assert "version probe skipped" in capsys.readouterr().out

    def multiple_addons_raise_with_fix_instructions():
        runner = mock.Mock()
        listdir = mock.Mock(return_value=["b.node", "a.node"])
        with pytest.raises(ProbeError) as exc_info:
            run("dist", "Cargo.toml", runner=runner, listdir=listdir)
        message = str(exc_info.value)
        assert "['a.node', 'b.node']" in message
        assert "release-ci.yml" in message
        runner.assert_not_called()

    def runs_the_addon_cli_version_through_node():
        runner = mock.Mock(return_value=_result(0, stdout="dirsql 0.4.72\n"))
        committed = mock.Mock(return_value="0.2.7")
        assert (
            run(
                "dist",
                "Cargo.toml",
                runner=runner,
                listdir=mock.Mock(return_value=[ADDON]),
                committed=committed,
            )
            == 0
        )
        runner.assert_called_once_with(
            ["node", "-e", PRINT_VERSION, os.path.abspath(os.path.join("dist", ADDON))],
            stdin=mock.ANY,
            capture_output=True,
            text=True,
        )
        assert runner.call_args.kwargs["stdin"] is not None
        committed.assert_called_once_with("Cargo.toml")

    def the_script_prints_the_cli_version_and_keeps_its_exit_code():
        assert PRINT_VERSION == (
            "process.exitCode = require(process.argv[1]).runCli(['--version'])"
        )

    def a_failing_cli_raises_with_its_output():
        runner = mock.Mock(return_value=_result(1, stdout="o", stderr="no addon"))
        with pytest.raises(ProbeError) as exc_info:
            _run_with(runner)
        assert str(exc_info.value) == (
            "`runCli(['--version'])` failed: stdout='o', stderr='no addon'"
        )

    def the_committed_literal_raises_the_unstamped_diagnosis():
        runner = mock.Mock(return_value=_result(0, stdout="dirsql 0.2.7\n"))
        with pytest.raises(ProbeError) as exc_info:
            _run_with(runner)
        message = str(exc_info.value)
        assert message.startswith(
            "the npm addon's CLI reports `dirsql 0.2.7`, the version committed in Cargo.toml"
        )
        assert "did not stamp the planned version" in message
        assert "putitoutthere.toml" in message

    def a_version_merely_containing_the_literal_passes():
        runner = mock.Mock(return_value=_result(0, stdout="dirsql 10.2.7\n"))
        assert _run_with(runner) == 0

    def success_reports_the_stamped_version(capsys):
        runner = mock.Mock(return_value=_result(0, stdout="dirsql 0.4.72\n"))
        assert _run_with(runner) == 0
        assert (
            f"ok npm-addon-version: {ADDON} reports `dirsql 0.4.72`"
            in capsys.readouterr().out
        )
