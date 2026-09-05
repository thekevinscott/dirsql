"""Colocated unit tests for reading a Python field off a TypeScript row
(isolation -- a dict in, a value out).
"""

from checks.platforms_mirror.typescript_value import DERIVE, typescript_value

TS_LINUX = {
    "triple": "x86_64-unknown-linux-gnu",
    "nodePlatform": "linux",
    "nodeArch": "x64",
    "libName": "@dirsql/lib-linux-x64-gnu",
    "os": ["linux"],
    "cpu": ["x64"],
    "libc": ["glibc"],
}


def describe_typescript_value():
    def it_strips_the_sub_package_prefix_off_the_slug():
        assert typescript_value("slug", TS_LINUX) == "linux-x64-gnu"

    def it_reads_other_fields_straight_through():
        assert typescript_value("cpu", TS_LINUX) == ["x64"]

    def it_leaves_a_lib_name_without_the_prefix_alone():
        assert typescript_value("slug", {"libName": "@dirsql/nope"}) == "@dirsql/nope"

    def it_derives_only_the_fields_that_need_it():
        assert set(DERIVE) == {"slug"}

    def it_does_not_strip_a_prefix_off_a_straight_read_field():
        row = {"nodePlatform": "@dirsql/lib-linux"}
        assert typescript_value("node_platform", row) == "@dirsql/lib-linux"

    def it_returns_none_for_a_missing_property():
        assert typescript_value("slug", {}) is None
