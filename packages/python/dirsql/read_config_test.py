"""Unit tests for `_read_config`.

The only collaborator is `open`, exercised against real temp files (the SUT is
the read itself, so faking it would leave nothing under test).
"""

import dirsql.read_config as mod


def describe_read_config():
    def it_returns_the_configs_text(tmp_path):
        config = tmp_path / "a.toml"
        config.write_text("path = 'x'\n", encoding="utf-8")
        assert mod._read_config(str(config)) == "path = 'x'\n"

    def it_decodes_as_utf8(tmp_path):
        config = tmp_path / "a.toml"
        config.write_text("path = 'café'\n", encoding="utf-8")
        assert mod._read_config(str(config)) == "path = 'café'\n"

    def it_returns_none_for_a_missing_config():
        assert mod._read_config("/nope/.dirsql.toml") is None

    def it_returns_none_for_a_directory(tmp_path):
        assert mod._read_config(str(tmp_path)) is None
