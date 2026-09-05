"""Colocated unit tests for reading an explicitly named workflow (#781)."""

import pytest

from checks.preflight.named_workflow import NoGateMatrix, REUSABLE, WORKFLOWS, named

CONVENTIONS = """
jobs:
  python-sdk:
    uses: x/y/.github/workflows/testing-conventions.yml@v0
    with:
      languages: '["python"]'
      source: packages/python/dirsql
      config: testing-conventions.toml
"""
NOT_A_CALLER = "jobs:\n  build:\n    runs-on: ubuntu-latest\n"


def reader(files):
    """A `read` that only knows the files it was given -- anything else raises."""
    return lambda path: files[path]


def describe_named():
    def it_returns_the_text_of_a_workflow_that_calls_the_reusable_workflow():
        assert named("wf.yml", reader({"wf.yml": CONVENTIONS})) == CONVENTIONS

    def it_names_the_file_and_the_fix_when_it_cannot_be_read():
        # The #973 regression exactly: the default target stopped existing and the
        # command died on a bare FileNotFoundError instead of saying what to do.
        def read(path):
            raise FileNotFoundError(2, "No such file or directory", path)

        with pytest.raises(NoGateMatrix) as caught:
            named(".github/workflows/conventions.yml", read)

        message = str(caught.value)
        assert "--conventions .github/workflows/conventions.yml: no such workflow" in message
        assert REUSABLE in message
        assert WORKFLOWS in message

    def it_rejects_a_workflow_that_declares_no_caller():
        # Accepting it would leave a green run whose matrix is empty.
        with pytest.raises(NoGateMatrix) as caught:
            named("docs.yml", reader({"docs.yml": NOT_A_CALLER}))

        message = str(caught.value)
        assert "--conventions docs.yml: no job in it calls" in message
        assert REUSABLE in message
        assert WORKFLOWS in message
