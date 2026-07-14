"""Unit tests for `discovered_fragments` (metadata + fragment_path mocked)."""

from unittest.mock import patch

from . import discovered_fragments as module
from .discovered_fragments import discovered_fragments


class _FakeEP:
    def __init__(self, name: str, value: str):
        self.name = name
        self.value = value


def describe_discovered_fragments():
    def it_orders_fragments_by_entry_point_name():
        eps = [_FakeEP("beta", "mod_b"), _FakeEP("alpha", "mod_a")]
        with (
            patch.object(
                module.metadata, "entry_points", return_value=eps
            ) as entry_points,
            patch.object(
                module, "fragment_path", side_effect=lambda m: f"/{m}/dirsql.toml"
            ),
        ):
            assert discovered_fragments() == [
                "/mod_a/dirsql.toml",
                "/mod_b/dirsql.toml",
            ]
        entry_points.assert_called_once_with(group="dirsql")
