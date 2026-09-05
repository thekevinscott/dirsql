"""Colocated unit tests for the node packaging distcheck gate (isolation).

Effects funnel through an injected `runner` (subprocess) and `fs` (FileSystem),
both mocked here; the detected host is passed in as a plain `Platform`.
"""
import json
from dataclasses import dataclass, field
from unittest import mock

import pytest

from distcheck.node_flow.gate import DistcheckError, run, staged_addon_path


@dataclass
class _Host:
    """Local stand-in for platforms.Platform -- the fields the gate reads."""

    slug: str = "linux-x64-gnu"
    addon_name: str = "dirsql.linux-x64-gnu.node"
    name: str = "@dirsql/lib-linux-x64-gnu"
    os: list[str] = field(default_factory=lambda: ["linux"])
    cpu: list[str] = field(default_factory=lambda: ["x64"])


_HOST = _Host()
_CLI_TGZ = "dirsql-lib-linux-x64-gnu-0.0.0-e2e.tgz"
_MAIN_TGZ = "dirsql-0.0.1.tgz"
_STAGED = "/ts/build/linux-x64-gnu/dirsql.linux-x64-gnu.node"
_BIN = "/inst/node_modules/.bin/dirsql"
_CLI_PKG = "/inst/node_modules/@dirsql/lib-linux-x64-gnu"


def _res(rc=0, stdout="", stderr=""):
    return mock.Mock(returncode=rc, stdout=stdout, stderr=stderr)


def _fs(exists=True, read_name="@dirsql/lib-linux-x64-gnu"):
    fs = mock.Mock()
    fs.mkdtemp.side_effect = ["/stg", "/inst"]
    fs.listdir.side_effect = [[_CLI_TGZ], [_CLI_TGZ, _MAIN_TGZ]]
    if callable(exists):
        fs.exists.side_effect = exists
    else:
        # A blanket True would also claim the retired `@dirsql/cli-<slug>`
        # package is installed, which the gate now treats as a failure; the
        # happy path is "everything present EXCEPT that".
        fs.exists.side_effect = lambda p: exists and "@dirsql/cli-" not in p
    fs.read_text.return_value = json.dumps({"name": read_name})
    return fs


def _ok_sequence():
    return [_res(), _res(), _res(), _res(stdout="dirsql 1.0")]


def test_run_success_executes_the_full_sequence():
    fs = _fs()
    runner = mock.Mock(side_effect=_ok_sequence())
    assert run("/ts", _HOST, runner=runner, fs=fs) == 0

    fs.copy.assert_called_once_with(_STAGED, "/stg/lib-pkg/dirsql.linux-x64-gnu.node")
    fs.chmod.assert_called_once_with("/stg/lib-pkg/dirsql.linux-x64-gnu.node", 0o755)

    cli_json = json.loads(fs.write_text.call_args_list[0].args[1])
    assert fs.write_text.call_args_list[0].args[0] == "/stg/lib-pkg/package.json"
    assert cli_json == {
        "name": "@dirsql/lib-linux-x64-gnu",
        "version": "0.0.0-e2e",
        "main": "dirsql.linux-x64-gnu.node",
        "os": ["linux"],
        "cpu": ["x64"],
    }

    install_json = json.loads(fs.write_text.call_args_list[1].args[1])
    assert fs.write_text.call_args_list[1].args[0] == "/inst/package.json"
    assert install_json == {
        "name": "dirsql-distcheck-host",
        "version": "0.0.0",
        "private": True,
    }

    # Full calls -- kwargs asserted too, so a flipped capture_output/text/cwd
    # does not survive.
    assert runner.call_args_list == [
        mock.call(
            ["npm", "pack", "--pack-destination", "/stg"],
            cwd="/stg/lib-pkg",
            capture_output=True,
            text=True,
        ),
        mock.call(
            ["pnpm", "pack", "--pack-destination", "/stg"],
            cwd="/ts",
            capture_output=True,
            text=True,
        ),
        mock.call(
            [
                "npm",
                "install",
                "--no-audit",
                "--no-fund",
                f"/stg/{_MAIN_TGZ}",
                f"/stg/{_CLI_TGZ}",
            ],
            cwd="/inst",
            capture_output=True,
            text=True,
        ),
        mock.call([_BIN, "--version"], capture_output=True, text=True),
    ]


def test_staged_addon_path_builds_the_pnpm_build_output_path():
    assert staged_addon_path("/ts", _HOST) == _STAGED


def test_run_missing_staged_binary_raises_prereq():
    fs = _fs(exists=lambda p: False)
    with pytest.raises(DistcheckError, match="prerequisite missing"):
        run("/ts", _HOST, runner=mock.Mock(), fs=fs)
    # The prerequisite `run` demands IS `staged_addon_path`'s output -- the
    # contract other callers guard on.
    fs.exists.assert_called_once_with(staged_addon_path("/ts", _HOST))
    fs.copy.assert_not_called()


def test_run_cli_pack_failure():
    fs = _fs()
    runner = mock.Mock(side_effect=[_res(rc=1, stdout="o", stderr="e")])
    with pytest.raises(DistcheckError, match="addon npm pack failed"):
        run("/ts", _HOST, runner=runner, fs=fs)


def test_run_main_pack_failure():
    fs = _fs()
    runner = mock.Mock(side_effect=[_res(), _res(rc=1, stdout="o", stderr="e")])
    with pytest.raises(DistcheckError, match="main pnpm pack failed"):
        run("/ts", _HOST, runner=runner, fs=fs)


def test_run_npm_install_failure():
    fs = _fs()
    runner = mock.Mock(side_effect=[_res(), _res(), _res(rc=1, stderr="e")])
    with pytest.raises(DistcheckError, match="npm install failed"):
        run("/ts", _HOST, runner=runner, fs=fs)


def test_run_bin_missing():
    fs = _fs(exists=lambda p: p != _BIN)
    runner = mock.Mock(side_effect=[_res(), _res(), _res()])
    with pytest.raises(DistcheckError, match="bin missing"):
        run("/ts", _HOST, runner=runner, fs=fs)


def test_run_version_nonzero_exit():
    fs = _fs()
    runner = mock.Mock(side_effect=[_res(), _res(), _res(), _res(rc=1, stdout="dirsql")])
    with pytest.raises(DistcheckError, match="--version` failed"):
        run("/ts", _HOST, runner=runner, fs=fs)


def test_run_version_missing_marker():
    fs = _fs()
    runner = mock.Mock(side_effect=[_res(), _res(), _res(), _res(stdout="nope")])
    with pytest.raises(DistcheckError, match="--version` failed"):
        run("/ts", _HOST, runner=runner, fs=fs)


def test_run_cli_subpkg_missing():
    fs = _fs(exists=lambda p: p != _CLI_PKG)
    runner = mock.Mock(side_effect=_ok_sequence())
    with pytest.raises(DistcheckError, match="addon sub-pkg missing"):
        run("/ts", _HOST, runner=runner, fs=fs)


def test_run_cli_subpkg_name_mismatch_greater():
    # A name lexicographically GREATER than host.name -- kills `!=` -> `<`.
    fs = _fs(read_name="@dirsql/lib-wrong")
    runner = mock.Mock(side_effect=_ok_sequence())
    with pytest.raises(DistcheckError, match="name mismatch"):
        run("/ts", _HOST, runner=runner, fs=fs)


def test_run_cli_subpkg_name_mismatch_lesser():
    # A name lexicographically LESS than host.name -- kills `!=` -> `>`.
    fs = _fs(read_name="@dirsql/lib-aaa")
    runner = mock.Mock(side_effect=_ok_sequence())
    with pytest.raises(DistcheckError, match="name mismatch"):
        run("/ts", _HOST, runner=runner, fs=fs)


def test_run_rejects_positional_runner():
    # `runner`/`fs` are keyword-only; a positional third arg must be rejected
    # (guards the `*` marker against a `/` positional-only mutation).
    with pytest.raises(TypeError):
        run("/ts", _HOST, mock.Mock())


def test_run_rejects_a_still_published_standalone_cli_subpkg():
    # The addon carries the CLI since #739. If a `@dirsql/cli-<slug>` package
    # reappears in an install, every consumer is carrying the core twice --
    # exactly the duplication this slice removed.
    runner = mock.Mock(side_effect=_ok_sequence())
    fs = _fs()
    fs.exists.side_effect = lambda p: True  # including the retired cli- path

    with pytest.raises(DistcheckError, match="standalone-CLI sub-package"):
        run("/ts", _HOST, runner=runner, fs=fs)
