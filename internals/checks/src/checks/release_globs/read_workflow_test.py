"""Colocated unit tests for reading a workflow file (#944)."""

from checks.release_globs.read_workflow import read_workflow


def describe_read_workflow():
    def it_parses_a_workflow_whose_on_key_yaml_resolves_to_a_boolean(tmp_path):
        path = tmp_path / "w.yml"
        path.write_text("on:\n  pull_request:\n    paths:\n      - 'a/**'\n")
        assert read_workflow(str(path))[True] == {"pull_request": {"paths": ["a/**"]}}
