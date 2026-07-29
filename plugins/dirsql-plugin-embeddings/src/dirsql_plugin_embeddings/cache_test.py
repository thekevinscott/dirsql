"""Colocated unit test for the shared cache singleton.

The singleton's field values are not restated here -- reading back what the
constructor was just handed pins nothing, and `read_pdf_test` already covers
the wiring that matters (the sub-cache's name, its directory, its inherited
defaults).

What is worth pinning is cachetta's `/`, which is load-bearing and not ours: it
must return a *derived* cache that keeps the singleton's settings. If an
upstream change made it drop them or mutate the singleton in place, every
cached function would silently rehome or change freshness.
"""

from .cache import cache


def describe_sub_caches():
    def it_derives_a_sub_cache_that_keeps_the_singletons_settings():
        sub = cache / "read_pdf"

        assert sub is not cache
        assert sub.path == cache.path / "read_pdf"
        assert (sub.hashed, sub.duration, sub.read) == (
            cache.hashed,
            cache.duration,
            cache.read,
        )

    def it_leaves_the_singleton_untouched():
        before = cache.path
        cache / "read_pdf"
        assert cache.path == before
