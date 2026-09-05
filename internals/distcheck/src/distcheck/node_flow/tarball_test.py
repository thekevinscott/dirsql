"""Colocated unit tests for tarball selection (a pure function, nothing to mock)."""
import pytest

from distcheck.node_flow.tarball import DistcheckError, select_tarball

_CLI_TGZ = "dirsql-lib-linux-x64-gnu-0.0.0-e2e.tgz"
_MAIN_TGZ = "dirsql-0.0.1.tgz"


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


def test_select_tarball_reports_the_matches_and_the_candidates():
    with pytest.raises(DistcheckError) as raised:
        select_tarball(
            ["a.tgz", "dirsql-lib-a.tgz", "dirsql-lib-b.tgz"], "dirsql-lib-"
        )
    message = str(raised.value)
    assert "'dirsql-lib-'" in message
    assert "['dirsql-lib-a.tgz', 'dirsql-lib-b.tgz']" in message
    assert "['a.tgz', 'dirsql-lib-a.tgz', 'dirsql-lib-b.tgz']" in message
