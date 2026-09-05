"""Colocated unit tests for the `libName` prefix verdict (isolation -- pure
dicts in, messages out).
"""

from checks.platforms_mirror.prefix_problems import prefix_problems

TS_LINUX = {
    "nodePlatform": "linux",
    "nodeArch": "x64",
    "libName": "@dirsql/lib-linux-x64-gnu",
}
TS_WIN = {
    "nodePlatform": "win32",
    "nodeArch": "x64",
    "libName": "@dirsql/lib-win32-x64-msvc",
}


def describe_prefix_problems():
    def it_passes_well_formed_names():
        assert prefix_problems([TS_LINUX, TS_WIN]) == []

    def it_names_a_lib_name_library_slug_would_throw_on():
        (message,) = prefix_problems([{**TS_WIN, "libName": "@dirsql/win32-x64-msvc"}])
        assert message.startswith("win32-x64: libName '@dirsql/win32-x64-msvc'")
        assert "@dirsql/lib-" in message

    def it_points_at_the_typescript_table_for_the_fix():
        (message,) = prefix_problems([{**TS_WIN, "libName": "nope"}])
        assert "packages/ts/src/platforms.ts" in message

    def it_names_a_row_with_no_lib_name_at_all():
        assert len(prefix_problems([{"nodePlatform": "win32", "nodeArch": "x64"}])) == 1
