"""Colocated unit tests for the python packaging distcheck gate (isolation).

The orchestrator's effects funnel through an injected `runner` (subprocess) and
`fs` (FileSystem); both are mocked here so the command sequence and every
stage's failure handling are exercised without a real build.
"""
import sys
from unittest import mock

import pytest

from distcheck.python_flow.gate import (
    DistcheckError,
    _require_zero,
    bin_subdir,
    check_wheel_tag,
    run,
    sole_wheel,
    wheel_version,
)

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
    # Both version stdouts carry the trailing newline a real process emits, so
    # the gate's `.strip()` is load-bearing rather than incidental.
    return [
        _res(),  # maturin build
        _res(),  # venv
        _res(),  # pip install
        _res(stdout="dirsql 1.0\n"),  # --version, matching _WHEEL's version
        _res(stdout="1.0\n"),  # import dirsql
    ]


def test_bin_subdir_is_scripts_on_windows_else_bin():
    assert bin_subdir("nt") == "Scripts"
    assert bin_subdir("posix") == "bin"


def test_require_zero_passes_on_success_and_raises_otherwise():
    _require_zero(_res(rc=0), "boom")  # no raise
    for rc in (1, -1):  # positive and signal-style negative exit codes
        with pytest.raises(DistcheckError, match="boom"):
            _require_zero(_res(rc=rc), "boom")


def test_run_rejects_positional_maturin():
    # `maturin`/`runner`/`fs` are keyword-only; a positional third arg must be
    # rejected (guards the `*` marker against a `/` positional-only mutation).
    with pytest.raises(TypeError):
        run("/pkg", "/repo", "maturin")


def test_sole_wheel_returns_the_single_wheel():
    assert sole_wheel(["notes.txt", _WHEEL]) == _WHEEL


def test_sole_wheel_rejects_none():
    with pytest.raises(DistcheckError, match="exactly one wheel"):
        sole_wheel(["notes.txt"])


def test_sole_wheel_rejects_many():
    with pytest.raises(DistcheckError, match="exactly one wheel"):
        sole_wheel(["a.whl", "b.whl"])


def test_wheel_version_reads_the_version_segment():
    assert wheel_version(_WHEEL) == "1.0"


def test_check_wheel_tag_accepts_abi3_cpython():
    check_wheel_tag(_WHEEL)  # no raise


def test_check_wheel_tag_rejects_non_abi3():
    with pytest.raises(DistcheckError, match="abi3 wheel tag"):
        check_wheel_tag("dirsql-1.0-cp311-cp311-linux_x86_64.whl")


def test_check_wheel_tag_rejects_non_cpython_interpreter():
    with pytest.raises(DistcheckError, match="interpreter tag"):
        check_wheel_tag("dirsql-1.0-xy-abi3-linux_x86_64.whl")


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


def test_run_version_disagrees_with_the_installed_wheel():
    # dirsql#958: the CLI printed the core crate's literal, not the version pip
    # installed. A "does it say dirsql" check passes that; this must not. Both
    # orderings, so an ordering comparison cannot pass for the equality.
    fs = _fs()
    for printed in ("dirsql 0.2.7\n", "dirsql 2.0\n"):
        runner = mock.Mock(
            side_effect=[_res(), _res(), _res(), _res(stdout=printed)]
        )
        with pytest.raises(DistcheckError, match="expected 'dirsql 1.0'"):
            run("/pkg", "/repo", runner=runner, fs=_fs())


def test_run_version_missing_marker():
    fs = _fs()
    runner = mock.Mock(side_effect=[_res(), _res(), _res(), _res(stdout="nope")])
    with pytest.raises(DistcheckError, match="expected 'dirsql 1.0'"):
        run("/pkg", "/repo", runner=runner, fs=fs)


def test_run_import_nonzero_exit():
    fs = _fs()
    runner = mock.Mock(
        side_effect=[_res(), _res(), _res(), _res(stdout="dirsql 1.0\n"), _res(rc=1)]
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
            _res(stdout="dirsql 1.0\n"),
            _res(stdout="   \n"),
        ]
    )
    with pytest.raises(DistcheckError, match="import dirsql` failed"):
        run("/pkg", "/repo", runner=runner, fs=fs)
