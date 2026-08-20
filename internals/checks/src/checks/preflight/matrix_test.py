"""Colocated unit tests for the CI gate-matrix parser (#781)."""

from checks.preflight.matrix import GATES, ROOT_GATES, parse_gate_matrix, pairs

PYTHON_CI = """
jobs:
  python-sdk:
    uses: x/y/.github/workflows/testing-conventions.yml@v0
    with:
      languages: '["python"]'
      source: packages/python/dirsql
      config: testing-conventions.toml
  unrelated:
    runs-on: ubuntu-latest
"""

RUST_CI = """
jobs:
  rust:
    uses: x/y/.github/workflows/testing-conventions.yml@v0
    with:
      languages: '["rust"]'
      source: packages/rust
      gates: '["colocated-test", "packaging"]'
"""

WORKFLOWS = [PYTHON_CI, RUST_CI]


def describe_parse_gate_matrix():
    def it_reads_each_callers_source_gates_and_config():
        entries = parse_gate_matrix(WORKFLOWS)
        assert [e.source for e in entries] == [
            "packages/python/dirsql",
            "packages/rust",
        ]
        assert entries[0].config == "testing-conventions.toml"
        assert entries[0].languages == ["python"]
        assert entries[1].gates == ["colocated-test", "packaging"]

    def it_collects_callers_across_every_workflow_in_document_order():
        # The matrix spans the per-domain workflows (#861 split `conventions.yml`
        # into one file per domain), so a caller in the last file is a pair too.
        assert [e.job for e in parse_gate_matrix(WORKFLOWS)] == ["python-sdk", "rust"]
        assert [e.job for e in parse_gate_matrix([RUST_CI, PYTHON_CI])] == [
            "rust",
            "python-sdk",
        ]

    def it_ignores_a_workflow_that_declares_no_jobs():
        # `.github/workflows/` holds non-gate workflows too; one without a `jobs:`
        # mapping must contribute nothing rather than raise.
        assert parse_gate_matrix(["on: push\n"]) == []

    def it_ignores_an_empty_workflow_document():
        assert parse_gate_matrix([""]) == []

    def it_defaults_gates_to_the_full_set_when_a_caller_omits_them():
        # An omitted `gates:` means the reusable workflow's default -- the full
        # set. Treating it as "no gates" would silently skip a root.
        (python, _rust) = parse_gate_matrix(WORKFLOWS)
        assert python.gates == ROOT_GATES

    def it_leaves_config_empty_when_a_caller_omits_it():
        (_python, rust) = parse_gate_matrix(WORKFLOWS)
        assert rust.config == ""

    def it_defaults_languages_to_empty_when_a_caller_omits_them():
        text = RUST_CI.replace("      languages: '[\"rust\"]'\n", "")
        (_python, rust) = parse_gate_matrix([PYTHON_CI, text])
        assert rust.languages == []

    def it_ignores_jobs_that_do_not_call_the_reusable_workflow():
        # `unrelated` sits AFTER a caller in its file, so skipping it must not
        # stop the walk at the first non-caller.
        assert [e.job for e in parse_gate_matrix([PYTHON_CI])] == ["python-sdk"]


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
            "unit-lint": ("unit", "lint"),
            "integration-lint": ("integration", "lint"),
            "unit-coverage": ("unit", "coverage"),
            "mutation": ("unit", "mutation"),
            "packaging": ("packaging",),
            "e2e-verify": ("e2e", "verify"),
        }


def describe_pairs():
    def it_crosses_every_root_language_and_gate():
        flat = [
            (r.job, lang, gate) for r, lang, gate in pairs(parse_gate_matrix(WORKFLOWS))
        ]
        assert flat == [
            *(("python-sdk", "python", gate) for gate in ROOT_GATES),
            ("rust", "rust", "colocated-test"),
            ("rust", "rust", "packaging"),
        ]

    def it_drops_a_root_that_declares_no_languages():
        text = RUST_CI.replace("'[\"rust\"]'", "'[]'")
        assert all(
            lang != "rust"
            for _root, lang, _gate in pairs(parse_gate_matrix([PYTHON_CI, text]))
        )
