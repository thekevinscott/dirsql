"""Unit tests for `binary_path`."""

from pathlib import Path
from unittest.mock import patch

import pytest

import dirsql_cli.binary_path as bp_module
from dirsql_cli.binary_path import binary_path


def _stage_binary(root: Path, name: str) -> Path:
    binary_dir = root / "_binary"
    binary_dir.mkdir()
    target = binary_dir / name
    target.write_text("")
    return target


class _SingleSegmentTraversable:
    """A `Traversable` whose `joinpath` takes exactly one child, as on 3.10.

    `pathlib.Path` accepts several, so the real-`Path` tests below pass either
    way; this catches a regression to the multi-segment form.
    """

    def __init__(self, path: Path):
        self._path = path

    def joinpath(self, child: str) -> "_SingleSegmentTraversable":
        return _SingleSegmentTraversable(self._path / child)

    def is_file(self) -> bool:
        return self._path.is_file()

    def __str__(self) -> str:
        return str(self._path)


def describe_binary_path():
    def it_returns_the_dirsql_path_on_posix(tmp_path: Path):
        target = _stage_binary(tmp_path, "dirsql")
        with (
            patch.object(bp_module, "is_windows", return_value=False),
            patch.object(bp_module, "files", return_value=tmp_path),
        ):
            assert binary_path() == str(target)

    def it_returns_the_dirsql_exe_path_on_windows(tmp_path: Path):
        target = _stage_binary(tmp_path, "dirsql.exe")
        with (
            patch.object(bp_module, "is_windows", return_value=True),
            patch.object(bp_module, "files", return_value=tmp_path),
        ):
            assert binary_path() == str(target)

    def it_walks_one_path_segment_at_a_time(tmp_path: Path):
        target = _stage_binary(tmp_path, "dirsql")
        with (
            patch.object(bp_module, "is_windows", return_value=False),
            patch.object(
                bp_module, "files", return_value=_SingleSegmentTraversable(tmp_path)
            ),
        ):
            assert binary_path() == str(target)

    def it_raises_file_not_found_when_the_posix_binary_is_missing(tmp_path: Path):
        with (
            patch.object(bp_module, "is_windows", return_value=False),
            patch.object(bp_module, "files", return_value=tmp_path),
            pytest.raises(FileNotFoundError, match="bundled `dirsql` not found"),
        ):
            binary_path()

    def it_raises_file_not_found_when_the_windows_binary_is_missing(tmp_path: Path):
        with (
            patch.object(bp_module, "is_windows", return_value=True),
            patch.object(bp_module, "files", return_value=tmp_path),
            pytest.raises(FileNotFoundError, match="bundled `dirsql.exe` not found"),
        ):
            binary_path()
