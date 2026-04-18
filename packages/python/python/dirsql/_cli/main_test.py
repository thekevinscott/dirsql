"""Unit tests for the ``main`` entry point."""

from __future__ import annotations

import os
import subprocess
import sys
import types

import pytest

import dirsql._cli.main as mod


def describe_main():
    def describe_on_unix():
        def it_execvs_and_forwards_argv(monkeypatch):
            monkeypatch.setattr(mod, "is_windows", lambda: False)
            monkeypatch.setattr(mod, "binary_path", lambda: "/fake/dirsql")

            seen: dict[str, object] = {}

            def fake_execv(path, args):
                seen["path"] = path
                seen["args"] = args
                raise SystemExit(0)

            monkeypatch.setattr(os, "execv", fake_execv)
            with pytest.raises(SystemExit):
                mod.main(["--port", "7117"])

            assert seen["path"] == "/fake/dirsql"
            assert seen["args"] == ["/fake/dirsql", "--port", "7117"]

    def describe_on_windows():
        def it_uses_subprocess_and_returns_the_child_exit_code(monkeypatch):
            monkeypatch.setattr(mod, "is_windows", lambda: True)
            monkeypatch.setattr(mod, "binary_path", lambda: "C:\\fake\\dirsql.exe")

            seen: dict[str, object] = {}

            def fake_run(cmd):
                seen["cmd"] = cmd
                return types.SimpleNamespace(returncode=42)

            monkeypatch.setattr(subprocess, "run", fake_run)
            code = mod.main(["serve"])
            assert code == 42
            assert seen["cmd"] == ["C:\\fake\\dirsql.exe", "serve"]

    def describe_default_argv():
        def it_falls_back_to_sys_argv_slice(monkeypatch):
            monkeypatch.setattr(mod, "is_windows", lambda: False)
            monkeypatch.setattr(mod, "binary_path", lambda: "/fake/dirsql")
            monkeypatch.setattr(sys, "argv", ["dirsql", "hello"])

            seen: dict[str, object] = {}

            def fake_execv(path, args):
                seen["args"] = args
                raise SystemExit(0)

            monkeypatch.setattr(os, "execv", fake_execv)
            with pytest.raises(SystemExit):
                mod.main(None)

            assert seen["args"] == ["/fake/dirsql", "hello"]

    def describe_when_the_binary_cannot_be_resolved():
        def it_returns_1_and_writes_a_dirsql_prefixed_stderr(
            capsys, monkeypatch
        ):
            def raise_missing():
                raise FileNotFoundError("bundled `dirsql` not found at /x")

            monkeypatch.setattr(mod, "binary_path", raise_missing)
            code = mod.main([])
            assert code == 1
            assert "dirsql: bundled `dirsql` not found" in capsys.readouterr().err
