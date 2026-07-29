"""Colocated unit test for `read_text`."""

from .read_text import read_text


def describe_read_text():
    def it_reads_file_contents(tmp_path):
        target = tmp_path / "note.md"
        target.write_text("on disk", encoding="utf-8")
        assert read_text(str(target)) == "on disk"

    def it_decodes_as_utf8(tmp_path):
        target = tmp_path / "accents.md"
        target.write_text("café ☕", encoding="utf-8")
        assert read_text(str(target)) == "café ☕"
