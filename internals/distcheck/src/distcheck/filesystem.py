"""A thin filesystem seam the distcheck orchestrators run through (#520).

The `python`/`node` flows are otherwise pure command-sequencing: funnelling every
effectful filesystem call through an injected `FileSystem` lets the unit tests
drive each stage with a mock (isolation) while the real implementation -- a set
of one-line delegations to `os`/`shutil`/`tempfile` -- is exercised directly
against temp dirs in `filesystem_test.py`.
"""
from __future__ import annotations

import os
import shutil
import tempfile


class FileSystem:
    def exists(self, path: str) -> bool:
        return os.path.exists(path)

    def makedirs(self, path: str) -> None:
        os.makedirs(path, exist_ok=True)

    def copy(self, src: str, dst: str) -> None:
        shutil.copy(src, dst)

    def chmod(self, path: str, mode: int) -> None:
        os.chmod(path, mode)

    def listdir(self, path: str) -> list[str]:
        return os.listdir(path)

    def mkdtemp(self, prefix: str) -> str:
        return tempfile.mkdtemp(prefix=prefix)

    def rmtree(self, path: str) -> None:
        shutil.rmtree(path, ignore_errors=True)

    def read_text(self, path: str) -> str:
        with open(path, encoding="utf-8") as handle:
            return handle.read()

    def write_text(self, path: str, data: str) -> None:
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(data)
