"""Colocated unit tests for the release-config reader (#790)."""

from checks.artifact_completeness.read_config import read_config


def describe_read_config():
    def it_parses_a_toml_config(tmp_path):
        path = tmp_path / "c.toml"
        path.write_text('[[package]]\nname = "x"\n')
        assert read_config(str(path)) == {"package": [{"name": "x"}]}
