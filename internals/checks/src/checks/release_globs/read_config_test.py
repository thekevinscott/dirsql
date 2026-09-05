"""Colocated unit tests for reading the release config (#944)."""

from checks.release_globs.read_config import read_config


def describe_read_config():
    def it_parses_a_toml_release_config(tmp_path):
        path = tmp_path / "putitoutthere.toml"
        path.write_text('[[package]]\nname = "x"\nglobs = ["a/**"]\n')
        assert read_config(str(path)) == {"package": [{"name": "x", "globs": ["a/**"]}]}
