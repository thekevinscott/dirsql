"""Colocated unit test for the plugin's cache configuration.

Only what can fail for a reason worth knowing about. Asserting a constant
equals the literal it was assigned pins nothing -- change the source and the
test changes with it -- so the constants are covered by the modules that
consume them, not restated here.

`CACHE_READ` is read from the environment at import, so these re-import the
module rather than patching after the fact.
"""

import importlib
import os
from datetime import timedelta
from unittest import mock

from . import config


def _reloaded(**env):
    with mock.patch.dict(os.environ, env, clear=True):
        return importlib.reload(config)


def describe_cache_duration():
    def it_outlives_cachettas_default():
        # (path, mtime) keying means a hit can never be stale, so an expiry
        # inside cachetta's 7-day default would only re-extract a file that
        # nothing has touched.
        assert config.CACHE_DURATION > timedelta(days=7)


def describe_cache_read():
    def it_reads_the_cache_by_default():
        assert _reloaded().CACHE_READ is True

    def it_stops_reading_when_the_variable_is_zero():
        assert _reloaded(**{config.ENV_CACHE_READ: "0"}).CACHE_READ is False

    def it_treats_any_other_value_as_enabled():
        assert _reloaded(**{config.ENV_CACHE_READ: "1"}).CACHE_READ is True


def teardown_module():
    # `_reloaded` mutated the imported module object; restore it so import
    # order cannot leak a patched environment into another test file.
    importlib.reload(config)
