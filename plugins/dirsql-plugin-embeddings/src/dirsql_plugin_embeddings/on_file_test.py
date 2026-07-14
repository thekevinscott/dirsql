"""Colocated unit tests for the on-file console script (isolation).

The embedder and the file read are mocked; stdout is captured. No real network
or file access.
"""

import json
from unittest import mock

from . import on_file as module
from .on_file import build_rows, main


def describe_build_rows():
    def it_builds_one_row_with_the_embedding_as_json_text():
        assert build_rows("/a/b.md", "hello", [1.0, 2.0]) == [
            {"path": "/a/b.md", "text": "hello", "embedding": "[1.0, 2.0]"}
        ]


def describe_main():
    def it_reads_embeds_and_prints_a_one_line_row_array(capsys):
        with (
            mock.patch.object(module, "_read_text", return_value="body text") as read,
            mock.patch.object(module, "embed", return_value=[0.5, 0.25]) as embed,
        ):
            assert main(["prog", "/abs/note.md"]) == 0
        read.assert_called_once_with("/abs/note.md")
        embed.assert_called_once_with("body text")
        out = capsys.readouterr().out
        assert out.endswith("\n")
        assert json.loads(out) == [
            {"path": "/abs/note.md", "text": "body text", "embedding": "[0.5, 0.25]"}
        ]

    def it_defaults_to_sys_argv(capsys):
        with (
            mock.patch.object(module.sys, "argv", ["prog", "/x.md"]),
            mock.patch.object(module, "_read_text", return_value="t"),
            mock.patch.object(module, "embed", return_value=[1.0]),
        ):
            assert main() == 0
        assert json.loads(capsys.readouterr().out)[0]["path"] == "/x.md"


def describe_read_text():
    def it_reads_file_contents(tmp_path):
        target = tmp_path / "note.md"
        target.write_text("on disk", encoding="utf-8")
        assert module._read_text(str(target)) == "on disk"
