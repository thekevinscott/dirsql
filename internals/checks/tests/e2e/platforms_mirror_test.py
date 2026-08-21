"""E2E test for `dirsql-checks platforms-mirror` through the real CLI.

No mocking of any kind: spawns the packaged `dirsql-checks` console script as a
subprocess, once against real scratch files and once against the repo's own
`platforms.py` / `platforms.ts` through the command's default paths.
"""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]

PYTHON_SOURCE = '''\
from dataclasses import dataclass


@dataclass(frozen=True)
class Platform:
    node_platform: str
    node_arch: str
    slug: str
    os: list[str]
    cpu: list[str]
{extra}

PLATFORMS: tuple[Platform, ...] = (
    Platform("linux", "x64", "linux-x64-gnu", ["linux"], ["x64"]{value}),
)
'''

TYPESCRIPT_SOURCE = """\
export const PLATFORMS: readonly Platform[] = [
  {
    triple: "x86_64-unknown-linux-gnu",
    nodePlatform: "linux",
    nodeArch: "x64",
    libName: "@dirsql/lib-linux-x64-gnu",
    os: ["linux"],
    cpu: ["x64"],
    libc: ["glibc"],
  },
];
"""


def _cli() -> str:
    dirsql_checks = shutil.which("dirsql-checks")
    assert dirsql_checks is not None, (
        "`dirsql-checks` console script not on PATH -- run "
        "`uv run --project internals/checks pytest tests/e2e` "
        "or `uv sync --project internals/checks`"
    )
    return dirsql_checks


def _write(tmp_path, *, stray_field: bool):
    python = tmp_path / "platforms.py"
    python.write_text(
        PYTHON_SOURCE.format(
            extra="    exe: bool\n" if stray_field else "",
            value=", False" if stray_field else "",
        )
    )
    typescript = tmp_path / "platforms.ts"
    typescript.write_text(TYPESCRIPT_SOURCE)
    return ["--python", str(python), "--typescript", str(typescript)]


def _invoke(arguments, cwd=None):
    return subprocess.run(
        [_cli(), "platforms-mirror", *arguments],
        capture_output=True,
        text=True,
        timeout=60,
        cwd=cwd,
    )


def describe_dirsql_checks_platforms_mirror():
    def it_exits_zero_when_the_subset_agrees(tmp_path):
        proc = _invoke(_write(tmp_path, stray_field=False))

        assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
        assert "ok platforms-mirror" in proc.stdout

    def it_exits_nonzero_and_names_the_unmirrored_field(tmp_path):
        proc = _invoke(_write(tmp_path, stray_field=True))

        assert proc.returncode == 1, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
        assert "Platform.exe" in proc.stdout

    def it_holds_the_repos_own_files_through_the_default_paths():
        proc = _invoke([], cwd=REPO)

        assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
