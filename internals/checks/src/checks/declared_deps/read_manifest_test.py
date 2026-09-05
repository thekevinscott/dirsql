"""Colocated unit tests for the declared-deps manifest reader (#782)."""

from checks.declared_deps.read_manifest import read_manifest


def describe_read_manifest():
    def it_parses_a_toml_manifest(tmp_path):
        path = tmp_path / "pyproject.toml"
        path.write_text('[project]\nname = "x"\n')
        assert read_manifest(str(path)) == {"project": {"name": "x"}}
