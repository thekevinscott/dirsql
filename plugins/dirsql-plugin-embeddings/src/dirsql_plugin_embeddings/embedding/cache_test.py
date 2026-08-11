from datetime import timedelta
from pathlib import Path
from unittest.mock import patch

from . import cache


def describe_cache_dir():
    def it_uses_xdg_cache_home_when_set():
        with patch.dict(cache.os.environ, {"XDG_CACHE_HOME": "/custom/cache"}):
            assert cache.cache_dir() == Path("/custom/cache/dirsql/embeddings")

    def it_falls_back_to_home_dot_cache_when_unset():
        with patch.dict(cache.os.environ, clear=True):
            with patch.object(cache.Path, "home", return_value=Path("/home/u")):
                assert cache.cache_dir() == Path("/home/u/.cache/dirsql/embeddings")

    def it_treats_an_empty_xdg_cache_home_as_unset():
        with patch.dict(cache.os.environ, {"XDG_CACHE_HOME": ""}):
            with patch.object(cache.Path, "home", return_value=Path("/home/u")):
                assert cache.cache_dir() == Path("/home/u/.cache/dirsql/embeddings")


def describe_make_cache():
    def it_builds_a_hashed_cachetta_at_the_cache_dir_with_no_eviction():
        with patch.object(cache, "Cachetta") as cachetta:
            with patch.object(
                cache, "cache_dir", return_value=Path("/somewhere")
            ) as directory:
                built = cache.make_cache()
        directory.assert_called_once_with()
        cachetta.assert_called_once_with(
            path=Path("/somewhere"), hashed=True, duration=cache.NO_EVICTION
        )
        assert built is cachetta.return_value

    def it_pins_no_eviction_to_a_thousand_years():
        assert cache.NO_EVICTION == timedelta(days=365000)
