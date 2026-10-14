"""Run one installed `dirsql server --frontend` through its browser workflow.

Fetches the page and script it serves, runs one query the way the page does,
then sends SIGTERM and requires the clean exit 0 of a graceful shutdown.
"""
from __future__ import annotations

import json
import subprocess
import urllib.request

from ..errors import DistcheckError

_ANNOUNCE = "Frontend at "


def check_server(
    cli: str,
    root: str,
    extra: list[str],
    sql: str,
    rows: list[dict],
    assets: dict[str, str],
    *,
    spawn=subprocess.Popen,
    request=urllib.request.urlopen,
) -> None:
    server = spawn(
        [cli, "server", "--frontend", "--port", "0", *extra],
        cwd=root,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        text=True,
    )
    try:
        announced = server.stdout.readline()
        if not announced.startswith(_ANNOUNCE):
            raise DistcheckError(
                f"`dirsql server --frontend` printed no frontend URL: {announced!r}"
            )
        url = announced.removeprefix(_ANNOUNCE).strip()
        for path, expected in assets.items():
            with request(url + path) as served:
                if served.read().decode() != expected:
                    raise DistcheckError(
                        f"{url + path} does not serve the bundled asset"
                    )
        query = urllib.request.Request(
            url + "query",
            data=json.dumps({"sql": sql}).encode(),
            headers={"content-type": "application/json"},
            method="POST",
        )
        with request(query) as answer:
            got = json.loads(answer.read())
        if got != rows:
            raise DistcheckError(f"{sql!r} returned {got!r}, expected {rows!r}")
    except BaseException:
        server.kill()
        server.wait()
        raise
    server.terminate()
    code = server.wait()
    if code != 0:
        raise DistcheckError(
            f"`dirsql server --frontend` exited {code} after SIGTERM, expected 0"
        )
