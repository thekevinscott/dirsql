import os
import sys
from unittest import mock

import pytest

from checks.wheel_extension_load.gate import (
    CONFIG,
    PROBE_SQL,
    ProbeError,
    bin_subdir,
    list_names,
    require_zero,
    run,
    wheel_names,
    write_text,
)

DIAGNOSE = "checks.wheel_extension_load.gate.diagnose"


def _result(returncode=0, stdout="", stderr=""):
    return mock.Mock(returncode=returncode, stdout=stdout, stderr=stderr)


def describe_collaborators():
    def it_resolves_the_probe_error_from_the_shared_probe_package():
        assert ProbeError.__module__ == "checks.probe.probe_error"

    def it_resolves_require_zero_from_the_shared_probe_package():
        assert require_zero.__module__ == "checks.probe.require_zero"

    def it_resolves_bin_subdir_from_the_shared_probe_package():
        assert bin_subdir.__module__ == "checks.probe.bin_subdir"

    def it_resolves_write_text_from_the_shared_probe_package():
        assert write_text.__module__ == "checks.probe.write_text"

    def it_resolves_list_names_from_its_own_module():
        assert list_names.__module__ == "checks.wheel_extension_load.list_names"


def describe_wheel_names():
    def filters_to_wheels_sorted():
        assert wheel_names(["b.whl", "notes.txt", "a.whl"]) == ["a.whl", "b.whl"]

    def empty_when_no_wheels():
        assert wheel_names(["a.tar.gz", "notes.txt"]) == []


def describe_run():
    def skips_cleanly_when_no_wheel(capsys):
        runner = mock.Mock()
        listdir = mock.Mock(return_value=["dirsql.tar.gz"])
        assert run("dist", runner=runner, listdir=listdir) == 0
        runner.assert_not_called()
        assert "probe skipped" in capsys.readouterr().out

    def multiple_wheels_raise_with_fix_instructions():
        runner = mock.Mock()
        listdir = mock.Mock(return_value=["b.whl", "a.whl"])
        with pytest.raises(ProbeError) as exc_info:
            run("dist", runner=runner, listdir=listdir)
        message = str(exc_info.value)
        assert "['a.whl', 'b.whl']" in message
        assert "release-precheck.yml" in message
        runner.assert_not_called()

    def probes_the_wheel_end_to_end():
        runner = mock.Mock(
            side_effect=[
                _result(0),
                _result(0),
                _result(0, stdout='[{"v":"v0.1.9"}]'),
            ]
        )
        listdir = mock.Mock(return_value=["dirsql-1.0.whl"])
        mkdtemp = mock.Mock(return_value="/staging")
        makedirs = mock.Mock()
        writer = mock.Mock()
        assert (
            run(
                "dist",
                runner=runner,
                listdir=listdir,
                mkdtemp=mkdtemp,
                makedirs=makedirs,
                writer=writer,
            )
            == 0
        )
        mkdtemp.assert_called_once_with("dirsql-extension-probe-")
        makedirs.assert_called_once_with(os.path.join("/staging", "data"))
        writer.assert_called_once_with(os.path.join("/staging", "ext.toml"), CONFIG)
        venv_bin = os.path.join("/staging", "venv", bin_subdir())
        assert runner.call_args_list[0] == mock.call(
            [sys.executable, "-m", "venv", os.path.join("/staging", "venv")],
            capture_output=True,
            text=True,
        )
        assert runner.call_args_list[1] == mock.call(
            [
                os.path.join(venv_bin, "pip"),
                "install",
                "--no-input",
                os.path.join("dist", "dirsql-1.0.whl"),
                "sqlite-vec",
            ],
            capture_output=True,
            text=True,
        )
        probe_args, probe_kwargs = runner.call_args_list[2]
        assert probe_args[0] == [
            os.path.join(venv_bin, "dirsql"),
            "query",
            PROBE_SQL,
            "--config",
            os.path.join("/staging", "ext.toml"),
            "--no-plugin",
        ]
        assert probe_kwargs["cwd"] == os.path.join("/staging", "data")
        assert probe_kwargs["capture_output"] is True
        assert probe_kwargs["text"] is True
        assert "stdin" in probe_kwargs

    def _run_with(runner):
        return run(
            "dist",
            runner=runner,
            listdir=mock.Mock(return_value=["dirsql-1.0.whl"]),
            mkdtemp=mock.Mock(return_value="/staging"),
            makedirs=mock.Mock(),
            writer=mock.Mock(),
        )

    def venv_failure_raises():
        runner = mock.Mock(return_value=_result(1, stderr="no venv"))
        with pytest.raises(ProbeError, match="venv creation failed:\nno venv"):
            _run_with(runner)

    def install_failure_raises():
        runner = mock.Mock(side_effect=[_result(0), _result(1, stderr="no dist")])
        with pytest.raises(ProbeError, match="pip install failed:\nno dist"):
            _run_with(runner)

    def failing_probe_raises_the_diagnosis():
        runner = mock.Mock(
            side_effect=[_result(0), _result(0), _result(1, stderr="load failed")]
        )
        with mock.patch(DIAGNOSE, return_value="the diagnosis") as diagnose:
            with pytest.raises(ProbeError, match="the diagnosis"):
                _run_with(runner)
        assert diagnose.call_args.args[0].stderr == "load failed"

    def signal_killed_probe_raises_the_diagnosis():
        runner = mock.Mock(
            side_effect=[_result(0), _result(0), _result(-6, stderr="load failed")]
        )
        with mock.patch(DIAGNOSE, return_value="the diagnosis"):
            with pytest.raises(ProbeError, match="the diagnosis"):
                _run_with(runner)

    def rowless_probe_output_raises():
        runner = mock.Mock(side_effect=[_result(0), _result(0), _result(0, stdout="[]")])
        with pytest.raises(ProbeError, match="probe query returned no row"):
            _run_with(runner)

    def success_reports_the_loaded_wheel(capsys):
        runner = mock.Mock(
            side_effect=[_result(0), _result(0), _result(0, stdout='[{"v":"v0.1.9"}]')]
        )
        assert _run_with(runner) == 0
        out = capsys.readouterr().out
        assert (
            'ok extension-load: dirsql-1.0.whl loaded sqlite-vec ([{"v":"v0.1.9"}])'
            in out
        )
