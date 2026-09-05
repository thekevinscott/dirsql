"""Colocated unit tests for the platforms-mirror decision logic (isolation -- pure
dicts in, messages out).
"""

from checks.platforms_mirror.decide import (
    DERIVE,
    SHARED,
    _slug,
    missing_rows,
    prefix_problems,
    stray_rows,
    typescript_value,
    unmirrored_fields,
)

FIELDS = list(SHARED)

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
TS_WIN = {
    "nodePlatform": "win32",
    "nodeArch": "x64",
    "libName": "@dirsql/lib-win32-x64-msvc",
    "os": ["win32"],
    "cpu": ["x64"],
}


def describe_unmirrored_fields():
    def it_passes_the_declared_subset():
        assert unmirrored_fields(FIELDS) == []

    def it_names_a_field_with_no_typescript_source():
        (message,) = unmirrored_fields([*FIELDS, "exe"])
        assert message.startswith("Platform.exe has no counterpart in ")
        assert "packages/ts/src/platforms.ts" in message

    def it_does_not_demand_the_typescript_only_fields():
        assert "triple" not in SHARED
        assert "libc" not in SHARED


def describe_typescript_value():
    def it_strips_the_sub_package_prefix_off_the_slug():
        assert typescript_value("slug", TS_LINUX) == "linux-x64-gnu"

    def it_reads_other_fields_straight_through():
        assert typescript_value("cpu", TS_LINUX) == ["x64"]

    def it_leaves_a_lib_name_without_the_prefix_alone():
        assert typescript_value("slug", {"libName": "@dirsql/nope"}) == "@dirsql/nope"

    def it_derives_only_the_fields_that_need_it():
        assert set(DERIVE) == {"slug"}
        assert _slug("@dirsql/lib-darwin-arm64") == "darwin-arm64"

    def it_does_not_strip_a_prefix_off_a_straight_read_field():
        row = {"nodePlatform": "@dirsql/lib-linux"}
        assert typescript_value("node_platform", row) == "@dirsql/lib-linux"

    def it_returns_none_for_a_missing_property():
        assert typescript_value("slug", {}) is None


def describe_prefix_problems():
    def it_passes_well_formed_names():
        assert prefix_problems([TS_LINUX, TS_WIN]) == []

    def it_names_a_lib_name_library_slug_would_throw_on():
        (message,) = prefix_problems([{**TS_WIN, "libName": "@dirsql/win32-x64-msvc"}])
        assert message.startswith("win32-x64: libName '@dirsql/win32-x64-msvc'")
        assert "@dirsql/lib-" in message

    def it_names_a_row_with_no_lib_name_at_all():
        assert len(prefix_problems([{"nodePlatform": "win32", "nodeArch": "x64"}])) == 1


def describe_missing_rows():
    def it_passes_when_every_published_target_has_a_row():
        assert missing_rows({"linux-x64"}, [TS_LINUX]) == []

    def it_suggests_the_row_to_add():
        (message,) = missing_rows(set(), [TS_LINUX])
        assert message.startswith("linux-x64 is published by platforms.ts")
        assert "Platform('linux', 'x64', 'linux-x64-gnu', ['linux'], ['x64'])" in message


def describe_stray_rows():
    def it_passes_when_every_row_is_published():
        assert stray_rows({"linux-x64"}, [PY_LINUX]) == []

    def it_names_a_row_the_release_never_publishes():
        (message,) = stray_rows(set(), [PY_LINUX])
        assert message.startswith("linux-x64 has a row in platforms.py")
        assert "packages/ts/src/platforms.ts" in message

