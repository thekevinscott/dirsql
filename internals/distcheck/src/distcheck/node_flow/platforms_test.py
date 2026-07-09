import pytest

from distcheck.node_flow.platforms import (
    PLATFORMS,
    detect_host,
    find_host_platform,
    to_node_arch,
)


def test_to_node_arch_maps_known_and_is_case_insensitive():
    assert to_node_arch("x86_64") == "x64"
    assert to_node_arch("AMD64") == "x64"
    assert to_node_arch("arm64") == "arm64"
    assert to_node_arch("aarch64") == "arm64"


def test_to_node_arch_rejects_unknown():
    with pytest.raises(ValueError, match="unsupported machine"):
        to_node_arch("mips")


def test_find_host_platform_linux_x64():
    p = find_host_platform("linux", "x64")
    assert p.slug == "linux-x64-gnu"
    assert p.name == "@dirsql/cli-linux-x64-gnu"
    assert p.bin_name == "dirsql"
    assert p.os == ["linux"]
    assert p.cpu == ["x64"]
    assert p.exe is False


def test_find_host_platform_arch_is_significant():
    # linux+arm64 must resolve to its own row, not linux-x64.
    assert find_host_platform("linux", "arm64").slug == "linux-arm64-gnu"


def test_find_host_platform_windows_uses_exe_suffix():
    p = find_host_platform("win32", "x64")
    assert p.exe is True
    assert p.bin_name == "dirsql.exe"


def test_find_host_platform_rejects_unknown():
    with pytest.raises(ValueError, match="unsupported host"):
        find_host_platform("plan9", "x64")


def test_detect_host_combines_arch_mapping_and_lookup():
    assert detect_host("darwin", "aarch64").slug == "darwin-arm64"


def test_every_platform_cli_name_matches_slug():
    for p in PLATFORMS:
        assert p.name == f"@dirsql/cli-{p.slug}"


def test_only_windows_rows_use_the_exe_suffix():
    for p in PLATFORMS:
        expected_exe = p.node_platform == "win32"
        assert p.exe is expected_exe
        assert p.bin_name == ("dirsql.exe" if expected_exe else "dirsql")


def test_platform_is_immutable():
    p = find_host_platform("linux", "x64")
    with pytest.raises(AttributeError):
        p.slug = "mutated"
