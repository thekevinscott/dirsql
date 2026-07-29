"""Colocated unit test for the shared cache singleton.

Reaching into cachetta's dataclass fields on purpose: the singleton's whole job
is to carry these defaults to every sub-cache, so reading them back is what
makes an upstream rename or a changed default fail loudly here rather than
silently in whichever module derives from it.
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
ENV_CACHE_READ = "DIRSQL_EMBEDDINGS_CACHE_READ"
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

    def it_lives_under_the_plugin_name():
        assert isinstance(cache.path, Path)
        assert cache.path.name == PLUGIN_NAME


def describe_disabling_reads():
    def it_reads_the_cache_by_default():
        assert _reloaded().read is True

    def it_stops_reading_when_the_variable_is_zero():
        assert _reloaded(**{ENV_CACHE_READ: "0"}).read is False

    def it_keeps_writing_when_reads_are_off():
        # Writes are harmless; it is a read of another run's leftovers that
        # would decide this run's result.
        assert _reloaded(**{ENV_CACHE_READ: "0"}).write is True


def describe_sub_caches():
    def it_scopes_a_named_sub_cache_under_the_cache_directory():
        assert (cache / "read_pdf").path == cache.path / "read_pdf"

    def it_keeps_the_singletons_defaults():
        sub = cache / "read_pdf"
        assert sub.hashed is True
        assert sub.duration == CACHE_DURATION
        assert sub.read == cache.read

    def it_leaves_the_singleton_untouched():
        before = cache.path
        cache / "read_pdf"
        assert cache.path == before


def teardown_module():
    # `_reloaded` mutated the imported module object; restore it so import
    # order cannot leak a patched environment into another test file.
    importlib.reload(module)
