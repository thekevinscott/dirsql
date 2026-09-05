"""Colocated unit tests for workflow discovery (#781)."""

import pytest

from checks.preflight.discovery import NoGateMatrix, discovered

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


def describe_discovered():
    FILES = {
        ".github/workflows/b-ci.yml": CONVENTIONS,
        ".github/workflows/a-ci.yaml": CONVENTIONS,
        ".github/workflows/docs.yml": NOT_A_CALLER,
    }

    def it_returns_every_caller_it_finds_in_sorted_order():
        found = discovered(
            ".github/workflows",
            lambda _directory: ["b-ci.yml", "docs.yml", "a-ci.yaml"],
            reader(FILES),
        )

        assert found == [
            (".github/workflows/a-ci.yaml", CONVENTIONS),
            (".github/workflows/b-ci.yml", CONVENTIONS),
        ]

    def it_never_reads_a_file_that_is_not_a_workflow():
        # `reader` raises for anything it was not given, so a README that got
        # opened fails here rather than passing quietly.
        found = discovered(
            ".github/workflows",
            lambda _directory: ["README.md", "b-ci.yml"],
            reader(FILES),
        )

        assert [path for path, _text in found] == [".github/workflows/b-ci.yml"]

    def it_names_the_directory_and_the_fix_when_it_does_not_exist():
        def listdir(directory):
            raise FileNotFoundError(2, "No such file or directory", directory)

        with pytest.raises(NoGateMatrix) as caught:
            discovered(".github/workflows", listdir, reader(FILES))

        assert "no .github/workflows directory here" in str(caught.value)
        assert "--conventions" in str(caught.value)

    def it_fails_when_no_workflow_in_the_directory_calls_the_reusable_workflow():
        with pytest.raises(NoGateMatrix) as caught:
            discovered(".github/workflows", lambda _directory: ["docs.yml"], reader(FILES))

        message = str(caught.value)
        assert "no workflow in .github/workflows calls" in message
        # The fix names where REUSABLE lives, which is matrix.py, not this module.
        assert "preflight/matrix.py" in message
