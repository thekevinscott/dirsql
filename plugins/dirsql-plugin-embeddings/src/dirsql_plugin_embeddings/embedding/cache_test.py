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
