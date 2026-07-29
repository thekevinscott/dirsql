"""Colocated unit test for the plugin's cache configuration."""

import os
from datetime import timedelta
from pathlib import Path
from unittest import mock

from . import config


def describe_cache_dir():
    def it_honors_an_explicit_override():
        with mock.patch.dict(os.environ, {config.ENV_CACHE_DIR: "/somewhere/else"}):
            assert config.cache_dir() == Path("/somewhere/else")

    def it_falls_back_to_xdg_cache_home_under_the_plugin_name():
        env = {config.ENV_XDG_CACHE_HOME: "/xdg"}
        with mock.patch.dict(os.environ, env, clear=True):
            assert config.cache_dir() == Path("/xdg") / config.PLUGIN_NAME

    def it_falls_back_to_dot_cache_when_xdg_is_unset():
        with (
            mock.patch.dict(os.environ, {}, clear=True),
            mock.patch.object(Path, "home", return_value=Path("/home/someone")),
        ):
            assert config.cache_dir() == Path("/home/someone/.cache") / config.PLUGIN_NAME

    def it_ignores_an_empty_override():
        env = {config.ENV_CACHE_DIR: "", config.ENV_XDG_CACHE_HOME: "/xdg"}
        with mock.patch.dict(os.environ, env, clear=True):
            assert config.cache_dir() == Path("/xdg") / config.PLUGIN_NAME


def describe_constants():
    def it_names_the_cache_after_the_distribution():
        assert config.PLUGIN_NAME == "dirsql-plugin-embeddings"

    def it_keeps_entries_far_past_cachettas_default():
        # (path, mtime) keying means a hit can never be stale, so expiry would
        # only re-extract a file nothing has touched.
        assert config.CACHE_DURATION == timedelta(days=365)
        assert config.CACHE_DURATION > timedelta(days=7)
