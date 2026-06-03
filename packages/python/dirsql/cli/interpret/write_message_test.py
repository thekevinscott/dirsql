"""Unit tests for `write_message`."""

import io
import sys
from unittest.mock import patch

from dirsql.cli.interpret.write_message import write_message


def describe_write_message():
    def it_serializes_a_dict_as_one_json_line():
        buf = io.StringIO()
        with patch.object(sys, "stdout", buf):
            write_message({"type": "config", "state": {}})
        assert buf.getvalue() == '{"type": "config", "state": {}}\n'

    def it_appends_a_trailing_newline():
        buf = io.StringIO()
        with patch.object(sys, "stdout", buf):
            write_message({"x": 1})
        assert buf.getvalue().endswith("\n")

    def it_flushes_stdout_after_writing():
        fake_stdout = io.StringIO()
        with (
            patch.object(sys, "stdout", fake_stdout),
            patch.object(fake_stdout, "flush") as flush,
        ):
            write_message({"x": 1})
            flush.assert_called_once()
