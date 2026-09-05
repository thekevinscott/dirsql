"""Colocated unit test for reading a platform table off disk (isolation -- a
scratch file, never the repo's real platforms.py / platforms.ts).
"""

from checks.platforms_mirror.read import read


def describe_read():
    def it_reads_a_file_as_utf8_text(tmp_path):
        path = tmp_path / "platforms.ts"
        path.write_text("// caffè\n", encoding="utf-8")
        assert read(str(path)) == "// caffè\n"

    def it_reads_the_whole_file(tmp_path):
        path = tmp_path / "platforms.py"
        path.write_text("one\ntwo\n", encoding="utf-8")
        assert read(str(path)) == "one\ntwo\n"
