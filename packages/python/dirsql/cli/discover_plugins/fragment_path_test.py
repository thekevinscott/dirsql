"""Unit tests for `fragment_path` (importlib.resources mocked)."""

from unittest.mock import patch

import pytest

from . import fragment_path as module
from .fragment_path import fragment_path


class _FakeFragment:
    def __init__(self, path: str, exists: bool):
        self._path = path
        self._exists = exists

    def is_file(self) -> bool:
        return self._exists

    def __str__(self) -> str:
        return self._path


class _FakeModuleDir:
    def __init__(self, fragment: _FakeFragment):
        self._fragment = fragment
        self.joined: str | None = None

    def joinpath(self, name: str) -> _FakeFragment:
        self.joined = name
        return self._fragment


def describe_fragment_path():
    def it_returns_the_shipped_fragment_path():
        module_dir = _FakeModuleDir(_FakeFragment("/abs/plug/dirsql.toml", True))
        with patch.object(module.resources, "files", return_value=module_dir) as files:
            assert fragment_path("plug") == "/abs/plug/dirsql.toml"
        files.assert_called_once_with("plug")
        assert module_dir.joined == "dirsql.toml"

    def it_raises_naming_the_module_when_not_importable():
        with patch.object(
            module.resources,
            "files",
            side_effect=ModuleNotFoundError("no module named plug"),
        ):
            with pytest.raises(ValueError, match="plug"):
                fragment_path("plug")

    def it_raises_naming_the_fragment_when_absent():
        module_dir = _FakeModuleDir(_FakeFragment("/abs/plug/dirsql.toml", False))
        with patch.object(module.resources, "files", return_value=module_dir):
            with pytest.raises(ValueError, match="dirsql.toml"):
                fragment_path("plug")
