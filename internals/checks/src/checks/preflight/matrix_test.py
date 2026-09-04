"""Colocated unit tests for the CI gate-matrix parser (#781)."""

from unittest import mock

import pytest

from checks.preflight.matrix import (
    GATES,
    NoGateMatrix,
    REUSABLE,
    ROOT_GATES,
    WORKFLOWS,
    discovered,
    named,
    pairs,
    parse_gate_matrix,
    read_text,
    sources,
)

CONVENTIONS = """
jobs:
  python-sdk:
    uses: x/y/.github/workflows/testing-conventions.yml@v0
    with:
      languages: '["python"]'
      source: packages/python/dirsql
      config: testing-conventions.toml
  unrelated:
    runs-on: ubuntu-latest
  rust:
    uses: x/y/.github/workflows/testing-conventions.yml@v0
    with:
      languages: '["rust"]'
      source: packages/rust
      gates: '["colocated-test", "packaging"]'
"""


def describe_parse_gate_matrix():
    def it_reads_each_callers_source_gates_and_config():
        entries = parse_gate_matrix(CONVENTIONS)
        assert [e.source for e in entries] == ["packages/python/dirsql", "packages/rust"]
        assert entries[0].config == "testing-conventions.toml"
        assert entries[0].languages == ["python"]
        assert entries[1].gates == ["colocated-test", "packaging"]

    def it_defaults_gates_to_the_full_set_when_a_caller_omits_them():
        # An omitted `gates:` means the reusable workflow's default -- the full
        # set. Treating it as "no gates" would silently skip a root.
        (python, _rust) = parse_gate_matrix(CONVENTIONS)
        assert python.gates == ROOT_GATES

    def it_leaves_config_empty_when_a_caller_omits_it():
        (_python, rust) = parse_gate_matrix(CONVENTIONS)
        assert rust.config == ""

    def it_defaults_languages_to_empty_when_a_caller_omits_them():
        text = CONVENTIONS.replace("      languages: '[\"rust\"]'\n", "")
        (_python, rust) = parse_gate_matrix(text)
        assert rust.languages == []

    def it_ignores_jobs_that_do_not_call_the_reusable_workflow():
        # `unrelated` sits BETWEEN two callers in the fixture, so skipping it must
        # not stop the walk at the first non-caller.
        assert [e.job for e in parse_gate_matrix(CONVENTIONS)] == ["python-sdk", "rust"]

    def it_reads_nothing_from_a_document_that_is_not_a_mapping():
        # Discovery hands it every file in the workflows directory, so a stray
        # list or scalar has to come back empty rather than raise.
        assert parse_gate_matrix("- one\n- two\n") == []

    def it_reads_nothing_from_a_workflow_that_declares_no_jobs():
        assert parse_gate_matrix("name: Docs\non: push\n") == []

    def it_reads_nothing_from_an_empty_jobs_key():
        assert parse_gate_matrix("jobs:\n") == []


def describe_GATES():
    def it_names_a_cli_subcommand_for_every_default_gate():
        assert sorted(GATES) == sorted(ROOT_GATES)

    def it_records_the_options_and_needs_of_every_gate():
        # Every flag in the table asserted at once: each is the difference between
        # a real run and an argv error or a silent false pass.
        shape = {
            name: (g.language, g.config, g.base, g.runs_suite, g.needs_artifact)
            for name, g in GATES.items()
        }
        assert shape == {
            "colocated-test": (True, True, True, False, False),
            "one-function-per-file": (True, True, False, False, False),
            "unit-lint": (True, True, False, False, False),
            "integration-lint": (True, True, False, False, False),
            "unit-coverage": (True, True, True, True, False),
            "mutation": (True, True, True, True, False),
            "packaging": (True, False, False, False, True),
            "e2e-verify": (False, False, True, False, False),
        }

    def it_maps_each_gate_to_its_cli_subcommand():
        assert {name: g.command for name, g in GATES.items()} == {
            "colocated-test": ("unit", "colocated-test"),
            "one-function-per-file": ("unit", "one-function-per-file"),
            "unit-lint": ("unit", "lint"),
            "integration-lint": ("integration", "lint"),
            "unit-coverage": ("unit", "coverage"),
            "mutation": ("unit", "mutation"),
            "packaging": ("packaging",),
            "e2e-verify": ("e2e", "verify"),
        }


def describe_pairs():
    def it_crosses_every_root_language_and_gate():
        flat = [(r.job, lang, gate) for r, lang, gate in pairs(parse_gate_matrix(CONVENTIONS))]
        assert flat == [
            *(("python-sdk", "python", gate) for gate in ROOT_GATES),
            ("rust", "rust", "colocated-test"),
            ("rust", "rust", "packaging"),
        ]

    def it_drops_a_root_that_declares_no_languages():
        text = CONVENTIONS.replace("'[\"rust\"]'", "'[]'")
        assert all(lang != "rust" for _root, lang, _gate in pairs(parse_gate_matrix(text)))


NOT_A_CALLER = "jobs:\n  build:\n    runs-on: ubuntu-latest\n"


def reader(files):
    """A `read` that only knows the files it was given -- anything else raises."""
    return lambda path: files[path]


def describe_read_text():
    def it_reads_a_workflow_as_utf8():
        with mock.patch("checks.preflight.matrix.open", mock.mock_open(read_data=CONVENTIONS)) as opened:
            assert read_text("wf.yml") == CONVENTIONS
        opened.assert_called_once_with("wf.yml", encoding="utf-8")


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

        assert "--conventions docs.yml: no job in it calls" in str(caught.value)


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
        assert f"no workflow in .github/workflows calls {REUSABLE}" in message
        assert "matrix.py" in message


def describe_sources():
    def it_reads_each_named_workflow_in_the_order_given():
        files = {"b.yml": CONVENTIONS, "a.yml": CONVENTIONS}

        assert sources(["b.yml", "a.yml"], read=reader(files)) == [
            ("b.yml", CONVENTIONS),
            ("a.yml", CONVENTIONS),
        ]

    def it_discovers_the_callers_when_no_workflow_is_named():
        found = sources(
            (),
            directory="wf",
            listdir=lambda _directory: ["ci.yml"],
            read=reader({"wf/ci.yml": CONVENTIONS}),
        )

        assert found == [("wf/ci.yml", CONVENTIONS)]

    def it_takes_its_seams_by_keyword_only():
        # `*` (not `/`) before the injected seams: a third positional argument
        # would otherwise land silently on `listdir`.
        with pytest.raises(TypeError):
            sources((), "wf", lambda _directory: ["ci.yml"], reader({"wf/ci.yml": CONVENTIONS}))

    def it_discovers_from_the_workflows_directory_by_default():
        # The default is the whole point: #834 deleted the workflow the command
        # used to name, and nothing pinned where it looks instead.
        looked = []

        sources(
            (),
            listdir=lambda directory: looked.append(directory) or ["ci.yml"],
            read=reader({".github/workflows/ci.yml": CONVENTIONS}),
        )

        assert looked == [WORKFLOWS] == [".github/workflows"]
