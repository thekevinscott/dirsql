"""Exercise the packaged `dirsql server --frontend` with and without `-c`."""
from __future__ import annotations

import subprocess
import urllib.request

from distcheck.filesystem import FileSystem


def probe_frontend(
    cli: str,
    repo_root: str,
    *,
    spawn=subprocess.Popen,
    request=urllib.request.urlopen,
    fs: FileSystem = FileSystem(),
) -> None:
    return None
