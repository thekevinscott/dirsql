"""Colocated unit tests for the platforms-mirror field comparison (isolation --
pure dicts in, messages out).
"""

from checks.platforms_mirror.field_problems import field_problems

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


def describe_field_problems():
    def it_passes_a_row_that_agrees():
        assert field_problems(PY_LINUX, TS_LINUX) == []

    def it_names_the_field_and_both_values():
        (message,) = field_problems({**PY_LINUX, "cpu": ["arm64"]}, TS_LINUX)
        assert message == (
            "linux-x64: cpu is ['arm64'] in platforms.py, ['x64'] in platforms.ts. "
            "platforms.ts is the release source of truth -- change "
            "internals/distcheck/src/distcheck/node_flow/platforms.py to match."
        )

    def it_reports_every_disagreeing_field():
        drifted = {**PY_LINUX, "cpu": ["arm64"], "slug": "nope"}
        assert len(field_problems(drifted, TS_LINUX)) == 2

    def it_reports_a_value_that_sorts_after_the_typescript_one():
        # The pair above drifts *below* platforms.ts; this one drifts above, so
        # neither ordering can stand in for the inequality.
        (message,) = field_problems({**PY_LINUX, "slug": "zzz"}, TS_LINUX)
        assert message.startswith("linux-x64: slug is 'zzz' in platforms.py")
