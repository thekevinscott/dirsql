"""Colocated unit test for `cache_dir` (isolation).

The environment and the home directory are both mocked -- resolving either for
real would make the result depend on the machine the suite runs on.
"""

import os
from pathlib import Path
from unittest import mock

from .cache_dir import CACHE_NAME, ENV_CACHE_DIR, ENV_XDG_CACHE_HOME, cache_dir


def _env(**values):
    return mock.patch.dict(os.environ, values, clear=True)


def describe_cache_dir():
    def it_uses_the_override_verbatim():
        with _env(**{ENV_CACHE_DIR: "/somewhere/else"}):
            assert cache_dir() == Path("/somewhere/else")

    def it_ignores_an_empty_override():
        with _env(**{ENV_CACHE_DIR: "", ENV_XDG_CACHE_HOME: "/xdg"}):
            assert cache_dir() == Path("/xdg") / CACHE_NAME

    def it_falls_back_to_the_xdg_cache_home():
        with _env(**{ENV_XDG_CACHE_HOME: "/xdg"}):
            assert cache_dir() == Path("/xdg") / CACHE_NAME

    def it_falls_back_to_dot_cache_under_home():
        with _env(), mock.patch.object(Path, "home", return_value=Path("/home/someone")):
            assert cache_dir() == Path("/home/someone/.cache") / CACHE_NAME

    def it_prefers_the_override_over_the_xdg_cache_home():
        with _env(**{ENV_CACHE_DIR: "/override", ENV_XDG_CACHE_HOME: "/xdg"}):
            assert cache_dir() == Path("/override")

    def it_names_the_directory_after_the_plugin():
        assert CACHE_NAME == "dirsql-plugin-embeddings"
