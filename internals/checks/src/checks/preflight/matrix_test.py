"""Colocated unit tests for the CI gate-matrix parser (#781)."""

from checks.preflight.matrix import GATES, ROOT_GATES, parse_gate_matrix, pairs

CONVENTIONS = """
jobs:
  python-sdk:
    uses: x/y/.github/workflows/testing-conventions.yml@v0
    with:
      languages: '["python"]'
      source: packages/python/dirsql
      config: testing-conventions.toml
  rust:
    uses: x/y/.github/workflows/testing-conventions.yml@v0
    with:
      languages: '["rust"]'
      source: packages/rust
      gates: '["colocated-test", "packaging"]'
  unrelated:
    runs-on: ubuntu-latest
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

    def it_leaves_config_unset_when_a_caller_omits_it():
        (_python, rust) = parse_gate_matrix(CONVENTIONS)
        assert rust.config is None

    def it_defaults_languages_to_empty_when_a_caller_omits_them():
        text = CONVENTIONS.replace("      languages: '[\"rust\"]'\n", "")
        (_python, rust) = parse_gate_matrix(text)
        assert rust.languages == []

    def it_ignores_jobs_that_do_not_call_the_reusable_workflow():
        assert [e.job for e in parse_gate_matrix(CONVENTIONS)] == ["python-sdk", "rust"]


def describe_GATES():
    def it_names_a_cli_subcommand_for_every_default_gate():
        assert sorted(GATES) == sorted(ROOT_GATES)

    def it_records_the_options_each_subcommand_actually_accepts():
        assert (GATES["packaging"].config, GATES["packaging"].base) == (False, False)
        assert GATES["e2e-verify"].language is False
        assert GATES["unit-lint"].base is False
        assert GATES["mutation"].base is True

    def it_flags_the_gates_that_execute_a_suite_or_need_a_built_artifact():
        assert [name for name, gate in GATES.items() if gate.runs_suite] == [
            *["unit-coverage", "mutation"]
        ]
        assert [name for name, gate in GATES.items() if gate.needs_artifact] == ["packaging"]


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
