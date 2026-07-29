"""Colocated unit test for the shared cache singleton.

Reaching into cachetta's dataclass fields on purpose: the singleton's whole job
is to carry these defaults to every sub-cache, so reading them back is what
makes an upstream rename or a changed default fail loudly here rather than
silently in whichever module derives from it.
"""

from datetime import timedelta
from pathlib import Path

from .cache import cache

# Literals, not imports from `config`: a unit test that reads the same
# constant it asserts pins nothing, and importing the collaborator breaks
# isolation.
PLUGIN_NAME = "dirsql-plugin-embeddings"
CACHE_DURATION = timedelta(days=365)


def describe_cache_singleton():
    def it_writes_one_entry_per_argument_set():
        assert cache.hashed is True

    def it_carries_the_configured_duration():
        assert cache.duration == CACHE_DURATION

    def it_lives_under_the_plugin_name():
        assert isinstance(cache.path, Path)
        assert cache.path.name == PLUGIN_NAME


def describe_reads_and_writes():
    def it_carries_the_configured_read_setting():
        assert cache.read is True

    def it_always_writes():
        # Writes are harmless; it is a read of another run's leftovers that
        # would decide this run's result, so only reads are switchable.
        assert cache.write is True


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
