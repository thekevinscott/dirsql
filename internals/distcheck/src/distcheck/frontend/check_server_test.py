"""Colocated unit tests for one packaged frontend server run (isolation).

The server process and the HTTP client are injected seams, both mocked here.
"""
import inspect
import json
import urllib.error
import urllib.request
from unittest import mock

import pytest

from distcheck.frontend.check_server import DistcheckError, check_server, subprocess

_URL = "http://localhost:4321/"
_ASSETS = {"": "<html>page</html>", "frontend.js": "export {};"}


def _server(announced=f"Frontend at {_URL}\n", code=0):
    server = mock.Mock()
    server.stdout.readline.return_value = announced
    server.wait.return_value = code
    return server


def _response(body):
    response = mock.MagicMock()
    response.__enter__.return_value.read.return_value = body.encode()
    return response


def _request(page="<html>page</html>", script="export {};", rows='[{"n": 1}]'):
    return mock.Mock(side_effect=[_response(page), _response(script), _response(rows)])


def _check(server, request, extra=(), spawn=None):
    spawn = spawn or mock.Mock(return_value=server)
    check_server(
        "/bin/dirsql",
        "/root",
        list(extra),
        "SELECT 1 AS n",
        [{"n": 1}],
        _ASSETS,
        spawn=spawn,
        request=request,
    )
    return spawn


def test_check_server_runs_the_browser_workflow_then_stops_cleanly():
    server = _server()
    request = _request()
    spawn = _check(server, request)

    spawn.assert_called_once_with(
        ["/bin/dirsql", "server", "--frontend", "--port", "0"],
        cwd="/root",
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        text=True,
    )
    assert request.call_args_list[0] == mock.call(_URL)
    assert request.call_args_list[1] == mock.call(f"{_URL}frontend.js")
    query = request.call_args_list[2].args[0]
    assert query.full_url == f"{_URL}query"
    assert query.get_method() == "POST"
    assert json.loads(query.data) == {"sql": "SELECT 1 AS n"}
    assert query.get_header("Content-type") == "application/json"
    server.terminate.assert_called_once_with()
    server.wait.assert_called_once_with()
    server.kill.assert_not_called()


def test_check_server_appends_the_extra_arguments():
    spawn = _check(_server(), _request(), extra=["-c", ".dirsql.toml"])

    assert spawn.call_args.args[0] == [
        "/bin/dirsql",
        "server",
        "--frontend",
        "--port",
        "0",
        "-c",
        ".dirsql.toml",
    ]


def test_check_server_rejects_a_missing_url_and_kills_the_server():
    server = _server(announced="Running at localhost:4321\n")
    request = _request()
    with pytest.raises(DistcheckError, match="printed no frontend URL"):
        _check(server, request)
    request.assert_not_called()
    server.kill.assert_called_once_with()
    server.wait.assert_called_once_with()
    server.terminate.assert_not_called()


def test_check_server_rejects_a_page_that_is_not_the_bundled_one():
    server = _server()
    with pytest.raises(DistcheckError, match=f"{_URL} does not serve the bundled asset"):
        _check(server, _request(page="<html>zzz</html>"))
    server.kill.assert_called_once_with()


def test_check_server_rejects_a_script_that_is_not_the_bundled_one():
    with pytest.raises(
        DistcheckError, match=f"{_URL}frontend.js does not serve the bundled asset"
    ):
        _check(_server(), _request(script="export const x = 1;"))


def test_check_server_rejects_unexpected_rows():
    with pytest.raises(DistcheckError, match=r"'SELECT 1 AS n' returned \[\{'n': 2\}\]"):
        _check(_server(), _request(rows='[{"n": 2}]'))


def test_check_server_kills_the_server_when_a_request_fails():
    server = _server()
    request = mock.Mock(side_effect=urllib.error.URLError("refused"))
    with pytest.raises(urllib.error.URLError):
        _check(server, request)
    server.kill.assert_called_once_with()
    server.wait.assert_called_once_with()


def test_check_server_rejects_an_unclean_shutdown():
    server = _server(code=1)
    with pytest.raises(DistcheckError, match="exited 1 after SIGTERM"):
        _check(server, _request())
    server.kill.assert_not_called()


def test_check_server_rejects_a_server_killed_by_the_signal():
    with pytest.raises(DistcheckError, match="exited -15 after SIGTERM"):
        _check(_server(code=-15), _request())


def test_check_server_defaults_to_real_processes_and_http():
    params = inspect.signature(check_server).parameters
    assert params["spawn"].default is subprocess.Popen
    assert params["request"].default is urllib.request.urlopen
    assert params["spawn"].kind is inspect.Parameter.KEYWORD_ONLY
