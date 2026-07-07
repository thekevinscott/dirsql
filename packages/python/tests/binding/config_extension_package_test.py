"""Binding tier: resolve a config-file extension by bare package name.

Lays a real compiled loadable inside a real package directory on `sys.path`,
declares it in a `.dirsql.toml` as a `[[dirsql.extension]]` entry whose `path`
is a bare **package name**, and asserts that constructing `DirSQL` from that
config resolves the actual on-disk file, loads it, and the function it
registers is callable. Real layout, no mocks.

The loadable is the repo's `tests/fixtures/testext` cdylib (registers
`dirsql_testext_answer() -> 42`), built on the fly with cargo.
"""

import importlib
import os
import shutil
import sys

import pytest

from dirsql import DirSQL

from .extension_package_test import _build_fixture_extension


def describe_config_extension_by_package_name():
    @pytest.mark.asyncio
    async def it_resolves_loads_and_calls_a_config_extension_by_package_name(
        tmp_path,
    ):
        if shutil.which(os.environ.get("CARGO", "cargo")) is None:
            pytest.skip("cargo not available to build the extension fixture")

        so = _build_fixture_extension(str(tmp_path / "target"))

        # A real installed-package layout:
        # <env>/dirsql_testext_pkg/{__init__.py, *.so}
        env = tmp_path / "env"
        pkg = env / "dirsql_testext_pkg"
        pkg.mkdir(parents=True)
        (pkg / "__init__.py").write_text("")
        shutil.copy(so, pkg / os.path.basename(so))

        # The config names the extension by bare package name; its root
        # defaults to the config file's parent directory.
        root = tmp_path / "root"
        root.mkdir()
        config = root / ".dirsql.toml"
        config.write_text(
            "[[dirsql.extension]]\n"
            'path = "dirsql_testext_pkg"\n'
            'entrypoint = "sqlite3_extension_init"\n'
        )

        sys.path.insert(0, str(env))
        importlib.invalidate_caches()
        try:
            db = DirSQL(config=str(config))
            await db.ready()
            assert await db.query("SELECT dirsql_testext_answer() AS a") == [{"a": 42}]
        finally:
            sys.path.remove(str(env))
            sys.modules.pop("dirsql_testext_pkg", None)
            importlib.invalidate_caches()
