"""Colocated unit tests for the python packaging distcheck gate (isolation).

The orchestrator's effects funnel through an injected `runner` (subprocess) and
`fs` (FileSystem); both are mocked here so the command sequence and every
stage's failure handling are exercised without a real build.
"""
import sys
from unittest import mock

import pytest

from distcheck.python_flow.gate import DistcheckError, bin_subdir, run

_WHEEL = "dirsql-1.0-cp311-abi3-linux_x86_64.whl"


def _res(rc=0, stdout="", stderr=""):
    return mock.Mock(returncode=rc, stdout=stdout, stderr=stderr)


def _fs(staging="/stg", listdir=None, exists=True):
    fs = mock.Mock()
    fs.mkdtemp.return_value = staging
    fs.listdir.return_value = [_WHEEL] if listdir is None else listdir
    if callable(exists):
        fs.exists.side_effect = exists
    else:
        fs.exists.return_value = exists
    return fs


def _ok_sequence():
    return [
        _res(),  # maturin build
        _res(),  # venv
        _res(),  # pip install
        _res(stdout="dirsql 1.0"),  # --version
        _res(stdout="1.0\n"),  # import dirsql
    ]


def test_bin_subdir_is_scripts_on_windows_else_bin():
    assert bin_subdir("nt") == "Scripts"
    assert bin_subdir("posix") == "bin"


def test_run_rejects_positional_maturin():
    # `maturin`/`runner`/`fs` are keyword-only; a positional third arg must be
    # rejected (guards the `*` marker against a `/` positional-only mutation).
    with pytest.raises(TypeError):
        run("/pkg", "/repo", "maturin")


def test_run_success_executes_the_full_sequence():
    fs = _fs()
    runner = mock.Mock(side_effect=_ok_sequence())
    # maturin defaulted (not passed) -- proves the default tool name is used.
    assert run("/pkg", "/repo", runner=runner, fs=fs) == 0

    # Nothing is staged into the package tree any more (#738): the wheel's
    # extension module carries the CLI, so there is no binary to copy in.
    fs.copy.assert_not_called()
    fs.chmod.assert_not_called()
    fs.rmtree.assert_called_once_with("/stg")

    # Full calls -- kwargs asserted too, so a flipped capture_output/text/cwd
    # does not survive.
    calls = runner.call_args_list
    assert calls[0] == mock.call(
        ["maturin", "build", "--out", "/stg/dist"],
        cwd="/pkg",
        capture_output=True,
        text=True,
    )
    assert calls[1] == mock.call(
        [sys.executable, "-m", "venv", "/stg/venv"], capture_output=True, text=True
    )
    assert calls[2] == mock.call(
        ["/stg/venv/bin/pip", "install", "--no-input", f"/stg/dist/{_WHEEL}"],
        capture_output=True,
        text=True,
    )
    # The version call additionally redirects stdin; assert its fields (the exact
    # DEVNULL value is not a mutation target, but capture_output/text are).
    assert calls[3].args[0] == ["/stg/venv/bin/dirsql", "--version"]
    assert calls[3].kwargs["capture_output"] is True
    assert calls[3].kwargs["text"] is True
    assert "stdin" in calls[3].kwargs
    assert calls[4] == mock.call(
        ["/stg/venv/bin/python", "-c", "import dirsql; print(dirsql.__version__)"],
        cwd="/stg",
        capture_output=True,
        text=True,
    )


def test_run_maturin_build_failure_still_cleans_up():
    fs = _fs()
    runner = mock.Mock(side_effect=[_res(rc=1, stdout="o", stderr="e")])
    with pytest.raises(DistcheckError, match="maturin build failed"):
        run("/pkg", "/repo", runner=runner, fs=fs)
    fs.rmtree.assert_called_once_with("/stg")


def test_run_venv_failure():
    fs = _fs()
    runner = mock.Mock(side_effect=[_res(), _res(rc=1, stderr="e")])
    with pytest.raises(DistcheckError, match="venv creation failed"):
        run("/pkg", "/repo", runner=runner, fs=fs)


def test_run_pip_install_failure():
    fs = _fs()
    runner = mock.Mock(side_effect=[_res(), _res(), _res(rc=1, stderr="e")])
    with pytest.raises(DistcheckError, match="pip install failed"):
        run("/pkg", "/repo", runner=runner, fs=fs)


def test_run_console_script_missing():
    fs = _fs(exists=lambda p: p != "/stg/venv/bin/dirsql")
    runner = mock.Mock(side_effect=[_res(), _res(), _res()])
    with pytest.raises(DistcheckError, match="console script missing"):
        run("/pkg", "/repo", runner=runner, fs=fs)


def test_run_version_nonzero_exit():
    fs = _fs()
    runner = mock.Mock(side_effect=[_res(), _res(), _res(), _res(rc=1, stdout="dirsql")])
    with pytest.raises(DistcheckError, match="--version` failed"):
        run("/pkg", "/repo", runner=runner, fs=fs)


def test_run_version_missing_marker():
    fs = _fs()
    runner = mock.Mock(side_effect=[_res(), _res(), _res(), _res(stdout="nope")])
    with pytest.raises(DistcheckError, match="--version` failed"):
        run("/pkg", "/repo", runner=runner, fs=fs)


def test_run_import_nonzero_exit():
    fs = _fs()
    runner = mock.Mock(
        side_effect=[_res(), _res(), _res(), _res(stdout="dirsql 1.0"), _res(rc=1)]
    )
    with pytest.raises(DistcheckError, match="import dirsql` failed"):
        run("/pkg", "/repo", runner=runner, fs=fs)


def test_run_import_empty_stdout():
    fs = _fs()
    runner = mock.Mock(
        side_effect=[
            _res(),
            _res(),
            _res(),
            _res(stdout="dirsql 1.0"),
            _res(stdout="   \n"),
        ]
    )
    with pytest.raises(DistcheckError, match="import dirsql` failed"):
        run("/pkg", "/repo", runner=runner, fs=fs)
