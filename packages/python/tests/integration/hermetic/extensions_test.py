"""Hermetic integration tests for the ``extensions=`` constructor kwarg.

These exercise the SDK public API with the Rust core mocked and every
filesystem / import-machinery probe faked: programmatic extension entries
flow through ``dirsql.resolve_extension`` (run for real) and must reach the
core resolved. Only the third-party boundaries are patched -- the PyO3 core
class and the effectful stdlib probes (``os.path.isfile``,
``importlib.util.find_spec``, ``glob.glob``). Real extension *loading* (a
missing ``.so`` failing ready, a fixture cdylib actually registering a
function) is covered by ``tests/binding/``.
"""

import glob
import importlib.util
import os
from types import SimpleNamespace
from typing import ClassVar
from unittest.mock import mock_open, patch

import pytest

from dirsql import _async as async_mod


class _FakeRustDirSQL:
    """Test double for the PyO3 ``DirSQL`` class, recording constructor kwargs."""

    instances: ClassVar[list] = []

    def __init__(
        self,
        root=None,
        *,
        extensions=None,
        suppress_config_extensions=False,
        **kwargs,
    ):
        self.root = root
        self.extensions = extensions
        self.suppress_config_extensions = suppress_config_extensions
        self.kwargs = kwargs
        _FakeRustDirSQL.instances.append(self)


@pytest.fixture(autouse=True)
def _reset_instances():
    _FakeRustDirSQL.instances = []
    yield
    _FakeRustDirSQL.instances = []


@pytest.fixture
def mock_core():
    """Replace the Rust-backed ``_RustDirSQL`` alias in ``dirsql._async``."""
    with patch.object(async_mod, "_RustDirSQL", _FakeRustDirSQL):
        yield _FakeRustDirSQL


@pytest.fixture
def mock_cwd():
    """Pin the cwd the SDK resolves programmatic entries against."""
    with patch.object(os, "getcwd", return_value="/cwd") as m:
        yield m


@pytest.fixture
def mock_isfile():
    """Fake the bare-name local-file shadow probe (no local file by default)."""
    with patch.object(os.path, "isfile", return_value=False) as m:
        yield m


@pytest.fixture
def mock_find_spec():
    """Fake the installed-package lookup behind bare-name resolution."""
    with patch.object(importlib.util, "find_spec") as m:
        yield m


@pytest.fixture
def mock_glob():
    """Fake the loadable-file glob inside a resolved package dir."""
    with patch.object(glob, "glob") as m:
        yield m


@pytest.fixture
def mock_config_file():
    """Fake a ``.dirsql.toml`` whose ``[[dirsql.extension]]`` names a package."""
    data = b'[[dirsql.extension]]\npath = "sqlite_vec"\n'
    with patch("builtins.open", mock_open(read_data=data)) as m:
        yield m


def describe_extensions_kwarg():
    # Feature: DirSQL(extensions=[{path, entrypoint?}]) loads SQLite
    # extensions at startup. See docs/howto/load-extension.md and
    # packages/python/README.md.
    @pytest.mark.asyncio
    async def it_defaults_extensions_to_none(mock_core):
        db = async_mod.DirSQL("/root", tables=["t"])
        await db.ready()

        assert _FakeRustDirSQL.instances[0].extensions is None

    @pytest.mark.asyncio
    async def it_forwards_literal_paths_verbatim(mock_core, mock_cwd):
        db = async_mod.DirSQL(
            "/root",
            tables=["t"],
            extensions=[{"path": "/abs/libvec.so", "entrypoint": "sqlite3_vec_init"}],
        )
        await db.ready()

        assert _FakeRustDirSQL.instances[0].extensions == [
            {"path": "/abs/libvec.so", "entrypoint": "sqlite3_vec_init"}
        ]

    @pytest.mark.asyncio
    async def it_defaults_a_missing_entrypoint_to_none(mock_core, mock_cwd):
        db = async_mod.DirSQL(
            "/root", tables=["t"], extensions=[{"path": "./rel/libvec.so"}]
        )
        await db.ready()

        assert _FakeRustDirSQL.instances[0].extensions == [
            {"path": "./rel/libvec.so", "entrypoint": None}
        ]

    @pytest.mark.asyncio
    async def it_resolves_a_bare_package_name_to_the_installed_loadable(
        mock_core, mock_cwd, mock_isfile, mock_find_spec, mock_glob
    ):
        mock_find_spec.return_value = SimpleNamespace(
            submodule_search_locations=["/site-packages/sqlite_vec"], origin=None
        )
        mock_glob.return_value = ["/site-packages/sqlite_vec/vec0.so"]

        db = async_mod.DirSQL(
            "/root", tables=["t"], extensions=[{"path": "sqlite_vec"}]
        )
        await db.ready()

        assert _FakeRustDirSQL.instances[0].extensions == [
            {"path": "/site-packages/sqlite_vec/vec0.so", "entrypoint": None}
        ]

    @pytest.mark.asyncio
    async def it_prefers_a_same_named_local_file_over_the_package(
        mock_core, mock_cwd, mock_isfile
    ):
        mock_isfile.return_value = True

        db = async_mod.DirSQL(
            "/root", tables=["t"], extensions=[{"path": "sqlite_vec"}]
        )
        await db.ready()

        assert _FakeRustDirSQL.instances[0].extensions == [
            {"path": os.path.join("/cwd", "sqlite_vec"), "entrypoint": None}
        ]

    @pytest.mark.asyncio
    async def it_resolves_config_extension_package_names_and_suppresses_core_loading(
        mock_core, mock_cwd, mock_isfile, mock_find_spec, mock_glob, mock_config_file
    ):
        mock_isfile.side_effect = lambda p: p == "/cfg/.dirsql.toml"
        mock_find_spec.return_value = SimpleNamespace(
            submodule_search_locations=["/site-packages/sqlite_vec"], origin=None
        )
        mock_glob.return_value = ["/site-packages/sqlite_vec/vec0.so"]

        db = async_mod.DirSQL(config="/cfg/.dirsql.toml")
        await db.ready()

        inst = _FakeRustDirSQL.instances[0]
        assert inst.extensions == [
            {"path": "/site-packages/sqlite_vec/vec0.so", "entrypoint": None}
        ]
        assert inst.suppress_config_extensions is True

    @pytest.mark.asyncio
    async def it_rejects_ready_when_a_bare_package_name_is_not_installed(
        mock_core, mock_cwd, mock_isfile, mock_find_spec
    ):
        mock_find_spec.return_value = None

        db = async_mod.DirSQL("/root", tables=["t"], extensions=[{"path": "nope"}])
        with pytest.raises(ValueError, match="not installed"):
            await db.ready()

        # The resolution error surfaced before the core was ever constructed.
        assert _FakeRustDirSQL.instances == []
