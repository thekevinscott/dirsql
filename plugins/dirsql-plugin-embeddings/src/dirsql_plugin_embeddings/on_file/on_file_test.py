"""Colocated unit test for the `on-file` entry point (isolation).

Every collaborator -- the file read, the row builder, the embedder -- is
mocked; stdout is captured. No real network or file access.
"""

import json
import sys
from unittest import mock

from .on_file import on_file

# `from . import on_file` yields the re-exported *function*, not this module:
# the barrel rebinds the submodule's name. Reach the module through the
# callable so the patches below land on the SUT's own globals.
module = sys.modules[on_file.__module__]


def describe_on_file():
    def it_reads_embeds_and_prints_a_one_line_row_array(capsys):
        with (
            mock.patch.object(module, "read_text", return_value="body text") as read,
            mock.patch.object(module, "embed", return_value=[0.5, 0.25]) as embed,
            mock.patch.object(
                module,
                "build_rows",
                return_value=[{"path": "/abs/note.md", "text": "body text"}],
            ) as build,
        ):
            assert on_file(["prog", "/abs/note.md"]) == 0
        read.assert_called_once_with("/abs/note.md")
        embed.assert_called_once_with("body text")
        build.assert_called_once_with("/abs/note.md", "body text", [0.5, 0.25])
        out = capsys.readouterr().out
        assert out.endswith("\n")
        assert json.loads(out) == [{"path": "/abs/note.md", "text": "body text"}]

    def it_defaults_to_sys_argv(capsys):
        with (
            mock.patch.object(module.sys, "argv", ["prog", "/x.md"]),
            mock.patch.object(module, "read_text", return_value="t"),
            mock.patch.object(module, "embed", return_value=[1.0]),
            mock.patch.object(module, "build_rows", return_value=[{"path": "/x.md"}]),
        ):
            assert on_file() == 0
        assert json.loads(capsys.readouterr().out)[0]["path"] == "/x.md"
