"""Colocated unit tests for the platforms-mirror gate (#1004).

Isolation: the reader is injected, so nothing here touches the repo's real
platforms.py / platforms.ts. The table readers and the verdict in
`problems.py` run for real, since they are pure text-in / messages-out.
"""

import inspect

from checks.platforms_mirror.gate import run

PYTHON = '''\
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

TYPESCRIPT = """\
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


def sources(python=None, typescript=TYPESCRIPT):
    text = {
        "p.py": PYTHON.format(extra="", value="") if python is None else python,
        "t.ts": typescript,
    }
    return lambda path: text[path]


def invoke(**kwargs):
    lines = []
    code = run("p.py", "t.ts", source=sources(**kwargs), echo=lines.append)
    return code, "\n".join(lines)


def describe_run():
    def it_reports_the_target_count_when_the_subset_agrees():
        code, out = invoke()
        assert code == 0
        assert out == (
            "ok platforms-mirror: p.py mirrors the shared fields of t.ts across "
            "1 published target(s)."
        )

    def it_fails_on_a_field_with_no_typescript_counterpart():
        code, out = invoke(python=PYTHON.format(extra="    exe: bool\n", value=", False"))
        assert code == 1
        assert "::error::Platform.exe has no counterpart" in out
        assert "platforms-mirror: 1 problem(s)." in out

    def it_names_vocabulary_py_so_the_shared_subset_can_be_found():
        code, out = invoke(python=PYTHON.format(extra="    exe: bool\n", value=", False"))
        assert code == 1
        assert "platforms_mirror/vocabulary.py's SHARED" in out

    def it_counts_every_problem_it_found():
        code, out = invoke(
            python=PYTHON.format(extra="    exe: bool\n    dev: bool\n", value=", False, True")
        )
        assert code == 1
        assert "platforms-mirror: 2 problem(s)." in out

    def it_fails_when_the_python_table_cannot_be_read():
        code, out = invoke(python="x = 1\n")
        assert code == 1
        assert "::error::platforms-mirror could not read a platform table:" in out
        assert "no `class Platform`" in out

    def it_fails_when_the_typescript_table_cannot_be_read():
        code, out = invoke(typescript="export const OTHER = [];\n")
        assert code == 1
        assert "could not read a platform table:" in out
        assert "no `PLATFORMS" in out

    def it_defaults_to_the_reader_in_its_own_module():
        default = inspect.signature(run).parameters["source"].default
        assert default.__module__ == "checks.platforms_mirror.read"
