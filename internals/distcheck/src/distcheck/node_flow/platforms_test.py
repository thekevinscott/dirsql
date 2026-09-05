import pytest

from distcheck.node_flow.platforms import PLATFORMS, detect_host, find_host_platform


def test_find_host_platform_linux_x64():
    p = find_host_platform("linux", "x64")
    assert p.slug == "linux-x64-gnu"
    assert p.name == "@dirsql/lib-linux-x64-gnu"
    assert p.addon_name == "dirsql.linux-x64-gnu.node"
    assert p.os == ["linux"]
    assert p.cpu == ["x64"]


def test_find_host_platform_arch_is_significant():
    # linux+arm64 must resolve to its own row, not linux-x64.
    assert find_host_platform("linux", "arm64").slug == "linux-arm64-gnu"


def test_find_host_platform_windows_addon_has_no_exe_suffix():
    # An addon is a `.node` on every platform; the `.exe` suffix belonged to the
    # standalone binary, which is no longer published (#739).
    p = find_host_platform("win32", "x64")
    assert p.addon_name == "dirsql.win32-x64-msvc.node"


def test_find_host_platform_rejects_unknown():
    with pytest.raises(ValueError, match="unsupported host"):
        find_host_platform("plan9", "x64")


def test_detect_host_combines_arch_mapping_and_lookup():
    assert detect_host("darwin", "aarch64").slug == "darwin-arm64"


def test_every_platform_addon_name_matches_slug():
    for p in PLATFORMS:
        assert p.name == f"@dirsql/lib-{p.slug}"
        assert p.addon_name == f"dirsql.{p.slug}.node"


def test_platform_is_immutable():
    p = find_host_platform("linux", "x64")
    with pytest.raises(AttributeError):
        p.slug = "mutated"


def test_platforms_covers_every_published_target():
    # PLATFORMS is read from packages/ts/src/platforms.json and `slug` is the
    # one field derived rather than carried; pin the whole set.
    assert sorted(p.slug for p in PLATFORMS) == [
        "darwin-arm64",
        "darwin-x64",
        "linux-arm64-gnu",
        "linux-x64-gnu",
        "win32-x64-msvc",
    ]
