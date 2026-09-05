"""Colocated unit tests for the shared platform vocabulary (isolation -- names
and pure lookups, no collaborators).
"""

from checks.platforms_mirror.vocabulary import (
    DERIVE,
    LIB_PREFIX,
    PYTHON_FILE,
    SHARED,
    TYPESCRIPT_FILE,
    key,
    slug,
)


def describe_shared():
    def it_names_every_mirrored_field_and_its_typescript_source():
        assert SHARED == {
            "node_platform": "nodePlatform",
            "node_arch": "nodeArch",
            "slug": "libName",
            "os": "os",
            "cpu": "cpu",
        }

    def it_leaves_the_typescript_only_fields_out_of_the_subset():
        assert "triple" not in SHARED
        assert "libc" not in SHARED
        assert "exe" not in SHARED


def describe_derive():
    def it_derives_only_the_slug():
        assert set(DERIVE) == {"slug"}
        assert DERIVE["slug"] is slug


def describe_slug():
    def it_strips_the_sub_package_prefix():
        assert slug("@dirsql/lib-darwin-arm64") == "darwin-arm64"

    def it_leaves_a_name_without_the_prefix_alone():
        assert slug("@dirsql/darwin-arm64") == "@dirsql/darwin-arm64"

    def it_reads_the_prefix_the_release_publishes_under():
        assert LIB_PREFIX == "@dirsql/lib-"


def describe_key():
    def it_joins_the_platform_and_the_arch_with_a_dash():
        assert key("linux", "x64") == "linux-x64"

    def it_keeps_the_platform_first():
        assert key("x64", "linux") == "x64-linux"


def describe_the_mirrored_files():
    def it_names_both_sides_of_the_mirror():
        assert PYTHON_FILE == "internals/distcheck/src/distcheck/node_flow/platforms.py"
        assert TYPESCRIPT_FILE == "packages/ts/src/platforms.ts"
