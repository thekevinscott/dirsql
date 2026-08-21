"""Integration tests for the platforms-mirror check against real files.

Exercises `gate.run` with its default collaborators -- a real `ast` parse of a
real `platforms.py` and a real parse of a real `platforms.ts` -- rather than the
packaged `dirsql-checks` CLI.

The reproduction is #1004 exactly: `Platform.exe` sat on the Python dataclass for
the stated reason that it "mirrors packages/ts/src/platforms.ts", while the
TypeScript source of truth has never carried the field. A comment asserted the
correspondence and nothing checked it, which is the same mechanism behind #972
and #996. The last case here is the one that matters day to day: the repo's own
two files, held to the invariant on every run.
"""

from __future__ import annotations

from pathlib import Path

from checks.platforms_mirror.gate import run

REPO = Path(__file__).resolve().parents[4]
PYTHON = REPO / "internals" / "distcheck" / "src" / "distcheck" / "node_flow" / "platforms.py"
TYPESCRIPT = REPO / "packages" / "ts" / "src" / "platforms.ts"

ANNOTATIONS = {
    "node_platform": "str",
    "node_arch": "str",
    "slug": "str",
    "os": "list[str]",
    "cpu": "list[str]",
    "exe": "bool",
}

SUBSET = ("node_platform", "node_arch", "slug", "os", "cpu")

LINUX = {
    "node_platform": "linux",
    "node_arch": "x64",
    "slug": "linux-x64-gnu",
    "os": ["linux"],
    "cpu": ["x64"],
    "exe": False,
}
WINDOWS = {
    "node_platform": "win32",
    "node_arch": "x64",
    "slug": "win32-x64-msvc",
    "os": ["win32"],
    "cpu": ["x64"],
    "exe": True,
}

PYTHON_SOURCE = '''\
from dataclasses import dataclass


@dataclass(frozen=True)
class Platform:
{fields}

    @property
    def name(self) -> str:
        return f"@dirsql/lib-{{self.slug}}"


PLATFORMS: tuple[Platform, ...] = (
{rows}
)
'''

TYPESCRIPT_SOURCE = """\
export interface Platform {{
  triple: string;
  nodePlatform: NodeJS.Platform;
  nodeArch: NodeJS.Architecture;
  libName: string;
  os: string[];
  cpu: string[];
  libc?: string[];
}}

export const PLATFORMS: readonly Platform[] = [
{rows}
];
"""


def write(tmp_path, python_rows, typescript_rows, fields=SUBSET):
    python = tmp_path / "platforms.py"
    python.write_text(
        PYTHON_SOURCE.format(
            fields="\n".join(f"    {name}: {ANNOTATIONS[name]}" for name in fields),
            rows="\n".join(
                f"    Platform({', '.join(repr(row[name]) for name in fields)}),"
                for row in python_rows
            ),
        )
    )
    typescript = tmp_path / "platforms.ts"
    typescript.write_text(
        TYPESCRIPT_SOURCE.format(
            rows="\n".join(
                "  {\n"
                f'    triple: "{row["node_arch"]}-{row["node_platform"]}",\n'
                f'    nodePlatform: "{row["node_platform"]}",\n'
                f'    nodeArch: "{row["node_arch"]}",\n'
                f'    libName: "{row.get("lib_name", "@dirsql/lib-" + row["slug"])}",\n'
                f'    os: {list(row["os"])!r},\n'
                f'    cpu: {list(row["cpu"])!r},\n'
                "  },"
                for row in typescript_rows
            ).replace("'", '"')
        )
    )
    return str(python), str(typescript)


def describe_run_against_real_platform_files():
    def it_passes_when_the_declared_subset_agrees(tmp_path, capsys):
        assert run(*write(tmp_path, [LINUX, WINDOWS], [LINUX, WINDOWS])) == 0
        assert "ok platforms-mirror" in capsys.readouterr().out

    def it_rejects_a_python_field_with_no_typescript_counterpart(tmp_path, capsys):
        # #1004: `exe` justified by a mirror that never had it.
        code = run(*write(tmp_path, [LINUX, WINDOWS], [LINUX, WINDOWS], fields=(*SUBSET, "exe")))

        assert code == 1
        out = capsys.readouterr().out
        assert "::error::Platform.exe" in out
        assert "no counterpart in packages/ts/src/platforms.ts" in out

    def it_rejects_a_published_target_missing_from_the_python_rows(tmp_path, capsys):
        # A new target half-added: published by the TS source of truth, invisible
        # to the node distcheck flow.
        code = run(*write(tmp_path, [LINUX], [LINUX, WINDOWS]))

        assert code == 1
        assert "::error::win32-x64 is published by platforms.ts" in capsys.readouterr().out

    def it_rejects_a_python_row_that_is_not_a_published_target(tmp_path, capsys):
        code = run(*write(tmp_path, [LINUX, WINDOWS], [LINUX]))

        assert code == 1
        assert "::error::win32-x64 has a row in platforms.py" in capsys.readouterr().out

    def it_rejects_a_shared_field_whose_values_disagree(tmp_path, capsys):
        drifted = {**WINDOWS, "cpu": ["arm64"]}
        code = run(*write(tmp_path, [LINUX, drifted], [LINUX, WINDOWS]))

        assert code == 1
        out = capsys.readouterr().out
        assert "::error::win32-x64: cpu is ['arm64'] in platforms.py" in out
        assert "['x64'] in platforms.ts" in out

    def it_rejects_a_lib_name_that_does_not_carry_the_sub_package_prefix(tmp_path, capsys):
        stray = {**WINDOWS, "lib_name": "@dirsql/win32-x64-msvc"}
        code = run(*write(tmp_path, [LINUX, WINDOWS], [LINUX, stray]))

        assert code == 1
        assert "@dirsql/lib-" in capsys.readouterr().out


def describe_run_against_the_repos_own_files():
    def it_holds_the_committed_platform_tables_to_the_invariant(capsys):
        assert run(str(PYTHON), str(TYPESCRIPT)) == 0, capsys.readouterr().out
