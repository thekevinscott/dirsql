"""Check whether a given package@version is already on a registry.

Used by the publish jobs to short-circuit when a prior run already landed
the release on that registry. crates.io and PyPI both forbid re-publish
at the same version, so without this probe a partial-failure retry gets
stuck on a 400.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Callable


@dataclass(frozen=True)
class HttpResponse:
    status: int
    body: str


def _default_http_get(url: str) -> HttpResponse:
    req = urllib.request.Request(url, headers={"User-Agent": "dirsql-release-scripts"})
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return HttpResponse(status=resp.status, body=resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        return HttpResponse(status=e.code, body="")


def check_published(
    *,
    registry: str,
    package: str,
    version: str,
    http_get: Callable[[str], HttpResponse] | None = None,
) -> bool:
    get = http_get or _default_http_get

    if registry == "crates":
        resp = get(f"https://crates.io/api/v1/crates/{package}")
        if resp.status == 404:
            return False
        if resp.status != 200:
            raise RuntimeError(f"crates.io returned status {resp.status}")
        data = json.loads(resp.body)
        return any(v.get("num") == version for v in data.get("versions", []))

    if registry == "pypi":
        resp = get(f"https://pypi.org/pypi/{package}/{version}/json")
        if resp.status == 200:
            return True
        if resp.status == 404:
            return False
        raise RuntimeError(f"pypi returned status {resp.status}")

    if registry == "npm":
        encoded = urllib.parse.quote(package, safe="")
        resp = get(f"https://registry.npmjs.org/{encoded}/{version}")
        if resp.status == 200:
            return True
        if resp.status == 404:
            return False
        raise RuntimeError(f"npm returned status {resp.status}")

    raise ValueError(f"unknown registry: {registry!r}")


def main() -> int:
    registry = os.environ["REGISTRY"]
    package = os.environ["PACKAGE"]
    version = os.environ["VERSION"]
    skip = check_published(registry=registry, package=package, version=version)
    line = f"skip={'true' if skip else 'false'}\n"
    out_path = os.environ.get("GITHUB_OUTPUT")
    if out_path:
        with open(out_path, "a", encoding="utf-8") as fh:
            fh.write(line)
    else:
        sys.stdout.write(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
