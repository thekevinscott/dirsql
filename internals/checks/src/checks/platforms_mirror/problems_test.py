"""Colocated unit tests for the full platforms-mirror verdict (isolation -- pure
dicts in, messages out).
"""

from checks.platforms_mirror.problems import problems

FIELDS = ["node_platform", "node_arch", "slug", "os", "cpu"]

PY_LINUX = {
    "node_platform": "linux",
    "node_arch": "x64",
    "slug": "linux-x64-gnu",
    "os": ["linux"],
    "cpu": ["x64"],
}
TS_LINUX = {
    "triple": "x86_64-unknown-linux-gnu",
    "nodePlatform": "linux",
    "nodeArch": "x64",
    "libName": "@dirsql/lib-linux-x64-gnu",
    "os": ["linux"],
    "cpu": ["x64"],
    "libc": ["glibc"],
}
PY_WIN = {
    "node_platform": "win32",
    "node_arch": "x64",
    "slug": "win32-x64-msvc",
    "os": ["win32"],
    "cpu": ["x64"],
}
TS_WIN = {
    "nodePlatform": "win32",
    "nodeArch": "x64",
    "libName": "@dirsql/lib-win32-x64-msvc",
    "os": ["win32"],
    "cpu": ["x64"],
}


def describe_problems():
    def it_passes_two_tables_that_mirror():
        assert problems(FIELDS, [PY_LINUX, PY_WIN], [TS_LINUX, TS_WIN]) == []

    def it_reports_the_structural_problems_before_the_field_ones():
        found = problems(
            [*FIELDS, "exe"],
            [PY_LINUX, {**PY_WIN, "cpu": ["arm64"]}],
            [TS_LINUX, TS_WIN],
        )
        assert found[0].startswith("Platform.exe")
        assert found[-1].startswith("win32-x64: cpu")

    def it_compares_only_the_rows_present_on_both_sides():
        found = problems(FIELDS, [PY_LINUX, PY_WIN], [TS_LINUX])
        assert len(found) == 1
        assert found[0].startswith("win32-x64 has a row in platforms.py")
