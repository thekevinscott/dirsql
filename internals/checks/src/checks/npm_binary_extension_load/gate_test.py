import os
import sys
from unittest import mock

import pytest

from checks.npm_binary_extension_load.gate import (
    BIN_NAME,
    ENTRYPOINT,
    PROBE_SQL,
    ProbeError,
    config_for,
    find_binaries,
    run,
)
from checks.wheel_extension_load.gate import bin_subdir

DIAGNOSE = "checks.npm_binary_extension_load.gate.diagnose"


def _result(returncode=0, stdout="", stderr=""):
    return mock.Mock(returncode=returncode, stdout=stdout, stderr=stderr)


def describe_find_binaries():
    def collects_matching_basenames_sorted():
        walker = mock.Mock(
            return_value=[
                ("dist/b", [], [BIN_NAME, "README.md"]),
                ("dist/a", [], [BIN_NAME]),
            ]
        )
        assert find_binaries("dist", walker) == [
            os.path.join("dist/a", BIN_NAME),
            os.path.join("dist/b", BIN_NAME),
        ]
        walker.assert_called_once_with("dist")

    def matches_by_value_not_identity():
        # A name `os.walk` hands back is a fresh string object, never the
        # interned BIN_NAME literal -- an identity comparison would find
        # nothing in the real artifact tree.
        name = "".join(["dir", "sql"])
        assert name == BIN_NAME
        assert name is not BIN_NAME
        walker = mock.Mock(return_value=[("dist", [], [name])])
        assert find_binaries("dist", walker) == [os.path.join("dist", BIN_NAME)]

    def ignores_other_names():
        walker = mock.Mock(return_value=[("dist", [], ["dirsql.exe", "notes.txt"])])
        assert find_binaries("dist", walker) == []

    def empty_walk_is_empty():
        walker = mock.Mock(return_value=[])
        assert find_binaries("dist", walker) == []


def describe_config_for():
    def declares_the_library_and_entrypoint():
        assert config_for("/v/vec0") == (
            '[[dirsql.extension]]\npath = "/v/vec0"\n'
            f'entrypoint = "{ENTRYPOINT}"\n'
        )


def describe_run():
    def skips_cleanly_when_no_binary(capsys):
        runner = mock.Mock()
        walker = mock.Mock(return_value=[("dist", [], ["notes.txt"])])
        assert run("dist", runner=runner, walker=walker) == 0
        runner.assert_not_called()
        assert "probe skipped" in capsys.readouterr().out

    def multiple_binaries_raise_with_fix_instructions():
        runner = mock.Mock()
        walker = mock.Mock(
            return_value=[("dist/a", [], [BIN_NAME]), ("dist/b", [], [BIN_NAME])]
        )
        with pytest.raises(ProbeError) as exc_info:
            run("dist", runner=runner, walker=walker)
        message = str(exc_info.value)
        assert "exactly one" in message
        assert "release-precheck.yml" in message
        runner.assert_not_called()

    def probes_the_binary_end_to_end():
        runner = mock.Mock(
            side_effect=[
                _result(0),
                _result(0),
                _result(0, stdout="/venv/site/sqlite_vec/vec0\n"),
                _result(0, stdout='[{"v":"v0.1.9"}]'),
            ]
        )
        walker = mock.Mock(return_value=[("dist", [], [BIN_NAME])])
        mkdtemp = mock.Mock(return_value="/staging")
        makedirs = mock.Mock()
        chmod = mock.Mock()
        writer = mock.Mock()
        abspath = mock.Mock(side_effect=lambda p: os.path.join("/abs", p))
        assert (
            run(
                "dist",
                runner=runner,
                walker=walker,
                mkdtemp=mkdtemp,
                makedirs=makedirs,
                chmod=chmod,
                writer=writer,
                abspath=abspath,
            )
            == 0
        )
        binary = os.path.join("/abs", "dist", BIN_NAME)
        chmod.assert_called_once_with(binary, 0o755)
        mkdtemp.assert_called_once_with("dirsql-npm-extension-probe-")
        makedirs.assert_called_once_with(os.path.join("/staging", "data"))
        writer.assert_called_once_with(
            os.path.join("/staging", "ext.toml"),
            config_for("/venv/site/sqlite_vec/vec0"),
        )
        venv_bin = os.path.join("/staging", "venv", bin_subdir())
        assert runner.call_args_list[0] == mock.call(
            [sys.executable, "-m", "venv", os.path.join("/staging", "venv")],
            capture_output=True,
            text=True,
        )
        assert runner.call_args_list[1] == mock.call(
            [os.path.join(venv_bin, "pip"), "install", "--no-input", "sqlite-vec"],
            capture_output=True,
            text=True,
        )
        assert runner.call_args_list[2] == mock.call(
            [
                os.path.join(venv_bin, "python"),
                "-c",
                "import sqlite_vec; print(sqlite_vec.loadable_path())",
            ],
            capture_output=True,
            text=True,
        )
        probe_args, probe_kwargs = runner.call_args_list[3]
        assert probe_args[0] == [
            binary,
            "query",
            PROBE_SQL,
            "--config",
            os.path.join("/staging", "ext.toml"),
        ]
        assert probe_kwargs["cwd"] == os.path.join("/staging", "data")
        assert probe_kwargs["capture_output"] is True
        assert probe_kwargs["text"] is True
        assert "stdin" in probe_kwargs

    def _run_with(runner):
        return run(
            "dist",
            runner=runner,
            walker=mock.Mock(return_value=[("dist", [], [BIN_NAME])]),
            mkdtemp=mock.Mock(return_value="/staging"),
            makedirs=mock.Mock(),
            chmod=mock.Mock(),
            writer=mock.Mock(),
            abspath=mock.Mock(side_effect=lambda p: os.path.join("/abs", p)),
        )

    def venv_failure_raises():
        runner = mock.Mock(return_value=_result(1, stderr="no venv"))
        with pytest.raises(ProbeError, match="venv creation failed:\nno venv"):
            _run_with(runner)

    def install_failure_raises():
        runner = mock.Mock(side_effect=[_result(0), _result(1, stderr="no wheel")])
        with pytest.raises(ProbeError, match="pip install failed:\nno wheel"):
            _run_with(runner)

    def locate_failure_raises():
        runner = mock.Mock(
            side_effect=[_result(0), _result(0), _result(1, stderr="no module")]
        )
        with pytest.raises(
            ProbeError, match="locating sqlite-vec's loadable library failed:\nno module"
        ):
            _run_with(runner)

    def failing_probe_raises_the_diagnosis():
        runner = mock.Mock(
            side_effect=[
                _result(0),
                _result(0),
                _result(0, stdout="/v/vec0\n"),
                _result(1, stderr="load failed"),
            ]
        )
        with mock.patch(DIAGNOSE, return_value="the diagnosis") as diagnose:
            with pytest.raises(ProbeError, match="the diagnosis"):
                _run_with(runner)
        assert diagnose.call_args.args[0].stderr == "load failed"

    def signal_killed_probe_raises_the_diagnosis():
        runner = mock.Mock(
            side_effect=[
                _result(0),
                _result(0),
                _result(0, stdout="/v/vec0\n"),
                _result(-6, stderr="load failed"),
            ]
        )
        with mock.patch(DIAGNOSE, return_value="the diagnosis"):
            with pytest.raises(ProbeError, match="the diagnosis"):
                _run_with(runner)

    def rowless_probe_output_raises():
        runner = mock.Mock(
            side_effect=[
                _result(0),
                _result(0),
                _result(0, stdout="/v/vec0\n"),
                _result(0, stdout="[]"),
            ]
        )
        with pytest.raises(ProbeError, match="probe query returned no row"):
            _run_with(runner)

    def success_reports_the_probed_binary(capsys):
        runner = mock.Mock(
            side_effect=[
                _result(0),
                _result(0),
                _result(0, stdout="/v/vec0\n"),
                _result(0, stdout='[{"v":"v0.1.9"}]'),
            ]
        )
        assert _run_with(runner) == 0
        out = capsys.readouterr().out
        binary = os.path.join("/abs", "dist", BIN_NAME)
        assert (
            f'ok npm-binary-extension-load: {binary} loaded sqlite-vec ([{{"v":"v0.1.9"}}])'
            in out
        )

    def runs_the_binary_by_absolute_path():
        # The probe execs with `cwd` set to a scratch dir, so a relative
        # --dist-dir (what release-precheck.yml passes) must be resolved
        # first or the exec dies with ENOENT against the scratch dir.
        runner = mock.Mock(
            side_effect=[
                _result(0),
                _result(0),
                _result(0, stdout="/v/vec0\n"),
                _result(0, stdout='[{"v":"v0.1.9"}]'),
            ]
        )
        assert _run_with(runner) == 0
        probe_argv = runner.call_args_list[3][0][0]
        assert os.path.isabs(probe_argv[0]), probe_argv[0]
