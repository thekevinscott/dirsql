"""Colocated unit tests for the unpublished-row verdict (isolation -- pure dicts
in, messages out).
"""

from checks.platforms_mirror.stray_rows import stray_rows

PY_LINUX = {
    "node_platform": "linux",
    "node_arch": "x64",
    "slug": "linux-x64-gnu",
    "os": ["linux"],
    "cpu": ["x64"],
}


def describe_stray_rows():
    def it_passes_when_every_row_is_published():
        assert stray_rows({"linux-x64"}, [PY_LINUX]) == []

    def it_names_a_row_the_release_never_publishes():
        (message,) = stray_rows(set(), [PY_LINUX])
        assert message.startswith("linux-x64 has a row in platforms.py")
        assert "packages/ts/src/platforms.ts" in message

    def it_names_both_files_the_disagreement_can_be_settled_in():
        (message,) = stray_rows(set(), [PY_LINUX])
        assert "internals/distcheck/src/distcheck/node_flow/platforms.py" in message
