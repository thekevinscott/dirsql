import os
import sys
from unittest import mock

import pytest

from checks.wheel_extension_load.gate import (
    CONFIG,
    PROBE_SQL,
    ProbeError,
    _require_zero,
    bin_subdir,
    list_names,
    run,
    wheel_names,
    write_text,
)

DIAGNOSE = "checks.wheel_extension_load.gate.diagnose"


def _result(returncode=0, stdout="", stderr=""):
    return mock.Mock(returncode=returncode, stdout=stdout, stderr=stderr)


def describe_require_zero():
    def zero_passes():
        assert _require_zero(_result(0), "boom") is None

    def positive_code_raises():
        with pytest.raises(ProbeError, match="boom"):
            _require_zero(_result(1), "boom")

    def negative_signal_code_raises():
        with pytest.raises(ProbeError, match="boom"):
            _require_zero(_result(-9), "boom")


def describe_bin_subdir():
    def windows_uses_scripts():
        assert bin_subdir("nt") == "Scripts"

    def posix_uses_bin():
        assert bin_subdir("posix") == "bin"


def describe_list_names():
    def returns_directory_entries():
        listdir = mock.Mock(return_value=["a.whl", "b.txt"])
        assert list_names("dist", listdir) == ["a.whl", "b.txt"]
        listdir.assert_called_once_with("dist")

    def missing_directory_is_empty():
        listdir = mock.Mock(side_effect=FileNotFoundError)
        assert list_names("dist", listdir) == []


def describe_wheel_names():
    def filters_to_wheels_sorted():
        assert wheel_names(["b.whl", "notes.txt", "a.whl"]) == ["a.whl", "b.whl"]

    def empty_when_no_wheels():
        assert wheel_names(["a.tar.gz", "notes.txt"]) == []


def describe_write_text():
    def writes_the_content(tmp_path):
        path = str(tmp_path / "ext.toml")
        write_text(path, "content")
        with open(path, encoding="utf-8") as handle:
            assert handle.read() == "content"


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
