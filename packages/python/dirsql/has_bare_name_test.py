"""Unit tests for `_has_bare_name`.

The one collaborator, the pure `is_bare_name`, is mocked so the probe's own
filtering -- non-dict entries, non-string paths, short-circuiting -- is what
these observe.
"""

from unittest import mock

import dirsql.has_bare_name as mod


def _patch():
    return mock.patch.object(
        mod, "is_bare_name", side_effect=lambda path: not path.endswith(".so")
    )


def describe_has_bare_name():
    def it_is_false_for_no_entries():
        with _patch() as is_bare_name:
            assert mod._has_bare_name([]) is False
            is_bare_name.assert_not_called()

    def it_is_false_when_every_path_is_literal():
        with _patch():
            assert mod._has_bare_name([{"path": "ext/a.so"}]) is False

    def it_is_true_when_some_path_is_a_bare_name():
        with _patch():
            assert mod._has_bare_name([{"path": "ext/a.so"}, {"path": "vec"}]) is True

    def it_skips_an_entry_that_is_not_a_table():
        with _patch() as is_bare_name:
            assert mod._has_bare_name([1, "vec"]) is False
            is_bare_name.assert_not_called()

    def it_skips_an_entry_whose_path_is_not_a_string():
        with _patch() as is_bare_name:
            assert mod._has_bare_name([{"path": 42}, {}]) is False
            is_bare_name.assert_not_called()
