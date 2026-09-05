"""Colocated unit tests for the unpublished-target verdict (isolation -- pure
dicts in, messages out).
"""

from checks.platforms_mirror.missing_rows import missing_rows

TS_LINUX = {
    "triple": "x86_64-unknown-linux-gnu",
    "nodePlatform": "linux",
    "nodeArch": "x64",
    "libName": "@dirsql/lib-linux-x64-gnu",
    "os": ["linux"],
    "cpu": ["x64"],
    "libc": ["glibc"],
}


def describe_missing_rows():
    def it_passes_when_every_published_target_has_a_row():
        assert missing_rows({"linux-x64"}, [TS_LINUX]) == []

    def it_suggests_the_row_to_add():
        (message,) = missing_rows(set(), [TS_LINUX])
        assert message.startswith("linux-x64 is published by platforms.ts")
        assert "Platform('linux', 'x64', 'linux-x64-gnu', ['linux'], ['x64'])" in message

    def it_points_at_the_python_table_it_would_be_added_to():
        (message,) = missing_rows(set(), [TS_LINUX])
        assert "internals/distcheck/src/distcheck/node_flow/platforms.py" in message
