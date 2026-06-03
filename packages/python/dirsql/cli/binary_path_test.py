"""Unit tests for `binary_path`."""

from pathlib import Path

import pytest

from dirsql.cli.binary_path import binary_path


def _stage_binary(root: Path, name: str) -> Path:
    binary_dir = root / "_binary"
    binary_dir.mkdir()
    target = binary_dir / name
    target.write_text("")
    return target


def describe_binary_path():
    def it_returns_the_dirsql_path_on_posix(tmp_path: Path):
        target = _stage_binary(tmp_path, "dirsql")
        assert binary_path(
            is_windows_fn=lambda: False,
            package_root=lambda: tmp_path,
        ) == str(target)

    def it_returns_the_dirsql_exe_path_on_windows(tmp_path: Path):
        target = _stage_binary(tmp_path, "dirsql.exe")
        assert binary_path(
            is_windows_fn=lambda: True,
            package_root=lambda: tmp_path,
        ) == str(target)

    def it_raises_file_not_found_when_the_posix_binary_is_missing(tmp_path: Path):
        with pytest.raises(FileNotFoundError, match="bundled `dirsql` not found"):
            binary_path(
                is_windows_fn=lambda: False,
                package_root=lambda: tmp_path,
            )

    def it_raises_file_not_found_when_the_windows_binary_is_missing(tmp_path: Path):
        with pytest.raises(FileNotFoundError, match="bundled `dirsql.exe` not found"):
            binary_path(
                is_windows_fn=lambda: True,
                package_root=lambda: tmp_path,
            )

    def it_defaults_to_the_installed_dirsql_package_resources():
        # The wheel's `_binary/dirsql` is staged at release time, not
        # checked into source. Calling `binary_path()` with default
        # `package_root` therefore exercises the real
        # `importlib.resources.files("dirsql")` wiring and falls into
        # the missing-binary branch -- which is exactly the production
        # behavior when a non-wheel install is used.
        with pytest.raises(FileNotFoundError, match="bundled `dirsql` not found"):
            binary_path(is_windows_fn=lambda: False)
