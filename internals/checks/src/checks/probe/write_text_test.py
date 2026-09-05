from checks.probe.write_text import write_text


def describe_write_text():
    def writes_the_content(tmp_path):
        path = str(tmp_path / "ext.toml")
        write_text(path, "content")
        with open(path, encoding="utf-8") as handle:
            assert handle.read() == "content"

    def it_replaces_an_existing_file(tmp_path):
        path = str(tmp_path / "ext.toml")
        write_text(path, "first")
        write_text(path, "second")
        with open(path, encoding="utf-8") as handle:
            assert handle.read() == "second"

    def it_encodes_as_utf8(tmp_path):
        path = str(tmp_path / "ext.toml")
        write_text(path, "caffè")
        with open(path, "rb") as handle:
            assert handle.read() == b"caff\xc3\xa8"
