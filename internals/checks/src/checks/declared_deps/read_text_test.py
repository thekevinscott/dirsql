"""Colocated unit tests for the declared-deps source reader (#782)."""

from checks.declared_deps.read_text import read_text


def describe_read_text():
    def it_reads_a_file_as_utf8(tmp_path):
        path = tmp_path / "s.py"
        path.write_text("# héllo\n", encoding="utf-8")
        assert read_text(str(path)) == "# héllo\n"
