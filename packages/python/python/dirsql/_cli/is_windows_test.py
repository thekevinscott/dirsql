"""Unit tests for ``is_windows``."""

from __future__ import annotations

from dirsql._cli import is_windows as mod


def describe_is_windows():
    def it_returns_false_on_posix(monkeypatch):
        monkeypatch.setattr(mod, "os", type("_", (), {"name": "posix"})())
        assert mod.is_windows() is False

    def it_returns_true_on_nt(monkeypatch):
        monkeypatch.setattr(mod, "os", type("_", (), {"name": "nt"})())
        assert mod.is_windows() is True
