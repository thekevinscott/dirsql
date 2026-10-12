"""Hermetic test that the watch case hands the harness a well-formed native."""

import os
from itertools import count
from unittest.mock import patch

import pytest

from tests.e2e.watch_speed_test import measure


class Side:
    def __init__(self, seconds):
        self.seconds = seconds
        self.samples = count(1)
        self.last = 0

    def sample(self):
        self.last = next(self.samples)
        return self.last, self.seconds


@pytest.fixture(params=[False, True], ids=["ratchet", "suite"])
def mode(request):
    env = {"DIRSQL_BENCH_PAIRS": "", "DIRSQL_BENCH_OUT": ""}
    if request.param:
        env = {"DIRSQL_BENCH_PAIRS": "3", "DIRSQL_BENCH_OUT": "/tmp/unused"}
    with patch.dict(os.environ, env):
        yield


def describe_measure():
    def it_pairs_the_watcher_with_dirsql(mode):
        native, dirsql = Side(1.0), Side(1.0)
        result = measure(native, dirsql)
        assert list(result.native_seconds) == ["inotifywait"]
        assert result.native_rows == {"inotifywait": native.last}
        assert result.dirsql_rows == dirsql.last
        assert result.bar == "inotifywait"
