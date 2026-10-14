"""Run one installed `dirsql server --frontend` through its browser workflow."""
from __future__ import annotations

import subprocess
import urllib.request

from ..errors import DistcheckError


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
    if spawn is None:
        raise DistcheckError("no server")
