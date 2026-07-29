"""Colocated unit test for `read_content` (isolation).

`read_text` is mocked: this unit owns the delegation, not the file read.
"""

from unittest import mock

from . import read_content as module
from .read_content import read_content


def describe_read_content():
    def it_returns_what_read_text_returns():
        with mock.patch.object(module, "read_text", return_value="body text"):
            assert read_content("/abs/note.md") == "body text"

    def it_delegates_the_path_unchanged():
        with mock.patch.object(module, "read_text", return_value="") as read_text:
            read_content("/abs/note.md")
        read_text.assert_called_once_with("/abs/note.md")
