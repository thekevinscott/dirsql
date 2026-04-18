"""Unit tests for ``binary_path``."""

from __future__ import annotations

import pytest

from dirsql._cli import binary_path as mod


class _FakeResource:
    def __init__(self, path: str, exists: bool) -> None:
        self._path = path
        self._exists = exists

    def is_file(self) -> bool:
        return self._exists

    def __str__(self) -> str:
        return self._path


class _FakeRoot:
    def __init__(self, resource_path: str, exists: bool) -> None:
        self.resource_path = resource_path
        self.exists = exists
        self.joinpath_calls: list[tuple[str, ...]] = []

    def joinpath(self, *parts: str) -> _FakeResource:
        self.joinpath_calls.append(parts)
        return _FakeResource(self.resource_path, self.exists)


def describe_binary_path():
    def describe_on_unix():
        def it_returns_the_resolved_dirsql_path(tmp_path, monkeypatch):
            monkeypatch.setattr(mod, "is_windows", lambda: False)
            binary = tmp_path / "dirsql"
            binary.write_text("#!/bin/sh\nexit 0\n")

            root = _FakeRoot(str(binary), exists=True)
            monkeypatch.setattr(mod, "files", lambda _mod: root)

            assert mod.binary_path() == str(binary)
            assert root.joinpath_calls == [("_binary", "dirsql")]

    def describe_on_windows():
        def it_looks_for_dirsql_exe(tmp_path, monkeypatch):
            monkeypatch.setattr(mod, "is_windows", lambda: True)
            binary = tmp_path / "dirsql.exe"
            binary.write_text("stub")

            root = _FakeRoot(str(binary), exists=True)
            monkeypatch.setattr(mod, "files", lambda _mod: root)

            assert mod.binary_path() == str(binary)
            assert root.joinpath_calls == [("_binary", "dirsql.exe")]

    def describe_when_the_binary_is_missing():
        def it_raises_FileNotFoundError_with_a_rebuild_hint(monkeypatch):
            monkeypatch.setattr(mod, "is_windows", lambda: False)
            root = _FakeRoot("/does/not/exist", exists=False)
            monkeypatch.setattr(mod, "files", lambda _mod: root)

            with pytest.raises(FileNotFoundError) as exc:
                mod.binary_path()
            assert "maturin build --release --bin dirsql --features cli" in str(
                exc.value
            )
