import os
import subprocess
import sys
from unittest import mock

import pytest

from checks.wheel_extension_load.gate import (
    CONFIG,
    PROBE_SQL,
    STATIC_MARKER,
    ProbeError,
    bin_subdir,
    diagnose,
    list_names,
    run,
    sole_wheel,
    write_text,
)


def _result(returncode=0, stdout="", stderr=""):
    return mock.Mock(returncode=returncode, stdout=stdout, stderr=stderr)


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


def describe_sole_wheel():
    def none_when_no_wheels():
        assert sole_wheel(["a.tar.gz", "notes.txt"]) is None

    def returns_the_single_wheel():
        assert sole_wheel(["dirsql-1.0-abi3.whl", "x.tar.gz"]) == "dirsql-1.0-abi3.whl"

    def multiple_wheels_raise_with_fix_instructions():
        with pytest.raises(ProbeError) as exc_info:
            sole_wheel(["b.whl", "a.whl"])
        message = str(exc_info.value)
        assert "['a.whl', 'b.whl']" in message
        assert "release-precheck.yml" in message


def describe_write_text():
    def writes_the_content(tmp_path):
        path = str(tmp_path / "ext.toml")
        write_text(path, "content")
        with open(path, encoding="utf-8") as handle:
            assert handle.read() == "content"


def describe_diagnose():
    def static_binary_gets_the_dlopen_diagnosis():
        message = diagnose(_result(1, stdout="", stderr=f"boom: {STATIC_MARKER}"))
        assert "statically linked" in message
        assert "dirsql#755" in message
        assert STATIC_MARKER in message

    def other_failures_get_a_generic_message():
        message = diagnose(_result(1, stdout="out", stderr="config missing"))
        assert message.startswith("`dirsql query` against the installed wheel failed.")
        assert "'config missing'" in message
        assert "'out'" in message


def describe_run():
    def skips_cleanly_when_no_wheel():
        runner = mock.Mock()
        listdir = mock.Mock(return_value=["dirsql.tar.gz"])
        assert run("dist", runner=runner, listdir=listdir) == 0
        runner.assert_not_called()

    def _happy_calls(probe_stdout='[{"v":"v0.1.9"}]'):
        return mock.Mock(
            side_effect=[
                _result(0),
                _result(0),
                _result(0, stdout=probe_stdout),
            ]
        )

    def probes_the_wheel_end_to_end():
        runner = _happy_calls()
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
        assert runner.call_args_list[2] == mock.call(
            [
                os.path.join(venv_bin, "dirsql"),
                "query",
                PROBE_SQL,
                "--config",
                os.path.join("/staging", "ext.toml"),
                "--no-plugin",
            ],
            cwd=os.path.join("/staging", "data"),
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
        )

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
            side_effect=[
                _result(0),
                _result(0),
                _result(1, stderr=f"failed to load extension: {STATIC_MARKER}"),
            ]
        )
        with pytest.raises(ProbeError) as exc_info:
            _run_with(runner)
        assert "statically linked" in str(exc_info.value)

    def rowless_probe_output_raises():
        runner = mock.Mock(
            side_effect=[_result(0), _result(0), _result(0, stdout="[]")]
        )
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
