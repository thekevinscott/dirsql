"""Colocated unit test for the shared cache singleton.

Reaching into cachetta's dataclass fields on purpose: the singleton's whole job
is to carry these defaults to every sub-cache, so reading them back is what
makes an upstream rename or a changed default fail loudly here rather than
silently in whichever module derives from it.

The singleton resolves its directory at import. `Cachetta.__truediv__` calls a
callable ``path`` and stores the result, so a lazy base would be frozen by the
first ``/`` anyway -- and the hooks run as fresh subprocesses, where import time
*is* process start. `_reloaded` re-imports under a patched environment to test
that resolution.
"""

import importlib
import os
from datetime import timedelta
from pathlib import Path
from unittest import mock

from . import cache as module
from .cache import cache


# Literals, not imports from `config`: a unit test that reads the same
# constant it asserts pins nothing, and importing the collaborator breaks
# isolation.
ENV_CACHE_DIR = "DIRSQL_EMBEDDINGS_CACHE_DIR"
ENV_XDG_CACHE_HOME = "XDG_CACHE_HOME"
PLUGIN_NAME = "dirsql-plugin-embeddings"
CACHE_DURATION = timedelta(days=365)


def _reloaded(**env):
    with mock.patch.dict(os.environ, env, clear=True):
        return importlib.reload(module).cache


def describe_cache_singleton():
    def it_writes_one_entry_per_argument_set():
        assert cache.hashed is True

    def it_carries_the_configured_duration():
        assert cache.duration == CACHE_DURATION

    def it_resolves_the_directory_rather_than_deferring_it():
        # A callable would be resolved by the first `/` regardless, so storing
        # one would imply a laziness the sub-caches do not have.
        assert isinstance(cache.path, Path)

    def it_reads_the_override_from_the_environment():
        assert _reloaded(**{ENV_CACHE_DIR: "/late/binding"}).path == Path(
            "/late/binding"
        )

    def it_falls_back_to_the_xdg_directory_under_the_plugin_name():
        reloaded = _reloaded(**{ENV_XDG_CACHE_HOME: "/xdg"})
        assert reloaded.path == Path("/xdg") / PLUGIN_NAME


def describe_sub_caches():
    def it_scopes_a_named_sub_cache_under_the_cache_directory():
        assert (cache / "read_pdf").path == cache.path / "read_pdf"

    def it_keeps_the_singletons_defaults():
        sub = cache / "read_pdf"
        assert sub.hashed is True
        assert sub.duration == CACHE_DURATION

    def it_leaves_the_singleton_untouched():
        before = cache.path
        cache / "read_pdf"
        assert cache.path == before


def teardown_module():
    # `_reloaded` mutated the imported module object; restore it so import
    # order cannot leak a patched environment into another test file.
    importlib.reload(module)
