"""Exercise the packaged `dirsql server --frontend` with and without `-c`.

The served page and script must equal the repo's bundled sources, so every
distribution is proven to carry the same UI with no download.
"""
from __future__ import annotations

import os
import subprocess
import urllib.request

from distcheck.filesystem import FileSystem

from .check_server import check_server

_ASSETS = {"": "frontend.html", "frontend.js": "frontend.js"}
_CONFIG = (
    "[[table]]\n"
    'name = "notes"\n'
    'ddl = "CREATE TABLE notes (title TEXT)"\n'
    'glob = "*.json"\n'
    'on-file = "cat"\n'
)


def probe_frontend(
    cli: str,
    repo_root: str,
    *,
    spawn=subprocess.Popen,
    request=urllib.request.urlopen,
    fs: FileSystem = FileSystem(),
) -> None:
    bundled = os.path.join(repo_root, "packages", "rust", "src", "cli")
    assets = {
        route: fs.read_text(os.path.join(bundled, name))
        for route, name in _ASSETS.items()
    }
    root = fs.mkdtemp("dirsql-frontend-")
    try:
        fs.write_text(os.path.join(root, "note.json"), '{"title": "hello"}\n')
        fs.write_text(os.path.join(root, ".dirsql.toml"), _CONFIG)
        check_server(
            cli,
            root,
            [],
            "SELECT basename FROM './' WHERE ext = 'json'",
            [{"basename": "note.json"}],
            assets,
            spawn=spawn,
            request=request,
        )
        check_server(
            cli,
            root,
            ["-c", ".dirsql.toml"],
            "SELECT title FROM notes",
            [{"title": "hello"}],
            assets,
            spawn=spawn,
            request=request,
        )
    finally:
        fs.rmtree(root)
