"""Colocated unit tests for the node packaging distcheck gate (isolation).

Effects funnel through an injected `runner` (subprocess) and `fs` (FileSystem),
both mocked here; the detected host is passed in as a plain `Platform`.
"""
import json
from dataclasses import dataclass, field
from unittest import mock

import pytest

from distcheck.node_flow.gate import (
    DistcheckError,
    _require_zero,
    run,
    select_tarball,
)


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
_MAIN_PKG_JSON = "/inst/node_modules/dirsql/package.json"
# The version npm installed -- `_MAIN_TGZ`'s, since that is the tarball
# installed above.
_INSTALLED = "0.0.1"


def _res(rc=0, stdout="", stderr=""):
    return mock.Mock(returncode=rc, stdout=stdout, stderr=stderr)


def _fs(exists=True, read_name="@dirsql/lib-linux-x64-gnu", installed=_INSTALLED):
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
    # Two installed manifests are read: the main package (for the version the
    # CLI must report) and the addon sub-package (for its name).
    fs.read_text.side_effect = lambda p: json.dumps(
        {"version": installed} if p == _MAIN_PKG_JSON else {"name": read_name}
    )
    return fs


def _ok_sequence():
    # The version stdout carries the trailing newline a real process emits, so
    # the gate's `.strip()` is load-bearing rather than incidental.
    return [_res(), _res(), _res(), _res(stdout=f"dirsql {_INSTALLED}\n")]


def test_select_tarball_finds_the_cli_tarball():
    assert select_tarball([_CLI_TGZ, _MAIN_TGZ], "dirsql-lib-") == _CLI_TGZ


def test_select_tarball_excludes_the_cli_tarball_for_the_main_prefix():
    assert (
        select_tarball([_CLI_TGZ, _MAIN_TGZ], "dirsql-", exclude="dirsql-lib-")
        == _MAIN_TGZ
    )


def test_select_tarball_rejects_none():
    with pytest.raises(DistcheckError, match="exactly one"):
        select_tarball(["other.tgz"], "dirsql-lib-")


def test_select_tarball_rejects_many():
    with pytest.raises(DistcheckError, match="exactly one"):
        select_tarball(["dirsql-lib-a.tgz", "dirsql-lib-b.tgz"], "dirsql-lib-")


def test_select_tarball_ignores_non_tgz():
    with pytest.raises(DistcheckError, match="exactly one"):
        select_tarball(["dirsql-lib-a.txt"], "dirsql-lib-")


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


def test_run_missing_staged_binary_raises_prereq():
    fs = _fs(exists=lambda p: False)
    with pytest.raises(DistcheckError, match="prerequisite missing"):
        run("/ts", _HOST, runner=mock.Mock(), fs=fs)
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


def test_run_version_disagrees_with_the_installed_package():
    # dirsql#958: the CLI printed the core crate's literal, not the version
    # npm installed. A "does it say dirsql" check passes that; this must not.
    fs = _fs()
    runner = mock.Mock(
        side_effect=[_res(), _res(), _res(), _res(stdout="dirsql 0.2.7\n")]
    )
    with pytest.raises(DistcheckError, match="expected 'dirsql 0.0.1'"):
        run("/ts", _HOST, runner=runner, fs=fs)


def test_run_version_older_than_the_installed_package():
    # A stale version can sort either side of the installed one; both are a
    # mismatch, and only testing one direction leaves an ordering comparison
    # indistinguishable from the equality this asserts.
    fs = _fs()
    runner = mock.Mock(
        side_effect=[_res(), _res(), _res(), _res(stdout="dirsql 0.0.0\n")]
    )
    with pytest.raises(DistcheckError, match="expected 'dirsql 0.0.1'"):
        run("/ts", _HOST, runner=runner, fs=fs)


def test_run_version_missing_marker():
    fs = _fs()
    runner = mock.Mock(side_effect=[_res(), _res(), _res(), _res(stdout="nope")])
    with pytest.raises(DistcheckError, match="expected 'dirsql 0.0.1'"):
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


def test_require_zero_passes_on_success_and_raises_otherwise():
    _require_zero(_res(rc=0), "boom")  # no raise
    for rc in (1, -1):  # positive and signal-style negative exit codes
        with pytest.raises(DistcheckError, match="boom"):
            _require_zero(_res(rc=rc), "boom")


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
