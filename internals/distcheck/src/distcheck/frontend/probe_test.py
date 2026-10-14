"""Colocated unit tests for the packaged frontend probe (isolation).

`check_server` and the filesystem are mocked; the probe's job is the fixture,
the expected bundled assets, and the two server runs.
"""
import inspect
import urllib.request
from unittest import mock

import pytest

from distcheck.frontend.probe import FileSystem, probe_frontend, subprocess

_CONFIG = (
    "[[table]]\n"
    'name = "notes"\n'
    'ddl = "CREATE TABLE notes (title TEXT)"\n'
    'glob = "*.json"\n'
    'on-file = "cat"\n'
)


def _fs():
    fs = mock.Mock()
    fs.mkdtemp.return_value = "/tree"
    fs.read_text.side_effect = lambda path: f"<{path}>"
    return fs


@pytest.fixture
def check_server():
    with mock.patch("distcheck.frontend.probe.check_server", create=True) as patched:
        yield patched


def test_probe_runs_the_server_without_and_with_a_config(check_server):
    fs = _fs()
    spawn, request = mock.Mock(), mock.Mock()

    probe_frontend("/bin/dirsql", "/repo", spawn=spawn, request=request, fs=fs)

    fs.read_text.assert_has_calls(
        [
            mock.call("/repo/packages/rust/src/cli/frontend.html"),
            mock.call("/repo/packages/rust/src/cli/frontend.js"),
        ]
    )
    fs.mkdtemp.assert_called_once_with("dirsql-frontend-")
    assert fs.write_text.call_args_list == [
        mock.call("/tree/note.json", '{"title": "hello"}\n'),
        mock.call("/tree/.dirsql.toml", _CONFIG),
    ]
    assets = {
        "": "</repo/packages/rust/src/cli/frontend.html>",
        "frontend.js": "</repo/packages/rust/src/cli/frontend.js>",
    }
    assert check_server.call_args_list == [
        mock.call(
            "/bin/dirsql",
            "/tree",
            [],
            "SELECT basename FROM './' WHERE ext = 'json'",
            [{"basename": "note.json"}],
            assets,
            spawn=spawn,
            request=request,
        ),
        mock.call(
            "/bin/dirsql",
            "/tree",
            ["-c", ".dirsql.toml"],
            "SELECT title FROM notes",
            [{"title": "hello"}],
            assets,
            spawn=spawn,
            request=request,
        ),
    ]
    fs.rmtree.assert_called_once_with("/tree")


def test_probe_removes_its_tree_when_a_run_fails(check_server):
    fs = _fs()
    check_server.side_effect = RuntimeError("boom")

    with pytest.raises(RuntimeError, match="boom"):
        probe_frontend("/bin/dirsql", "/repo", fs=fs)
    fs.rmtree.assert_called_once_with("/tree")


def test_probe_defaults_to_real_processes_and_http():
    params = inspect.signature(probe_frontend).parameters
    assert params["spawn"].default is subprocess.Popen
    assert params["request"].default is urllib.request.urlopen
    assert isinstance(params["fs"].default, FileSystem)
