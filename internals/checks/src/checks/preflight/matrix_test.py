"""Colocated unit tests for the CI gate-matrix parser (#781)."""

from checks.preflight.matrix import ROOT_GATES, parse_gate_matrix

CONVENTIONS = """
jobs:
  python-sdk:
    uses: x/y/.github/workflows/testing-conventions.yml@v0
    with:
      source: packages/python/dirsql
      config: testing-conventions.toml
  rust:
    uses: x/y/.github/workflows/testing-conventions.yml@v0
    with:
      source: packages/rust
      gates: '["colocated-test", "packaging"]'
  unrelated:
    runs-on: ubuntu-latest
"""


def describe_parse_gate_matrix():
    def it_reads_each_callers_source_and_gates():
        entries = parse_gate_matrix(CONVENTIONS)
        assert [e.source for e in entries] == ["packages/python/dirsql", "packages/rust"]
        assert entries[1].gates == ["colocated-test", "packaging"]

    def it_defaults_gates_to_the_full_set_when_a_caller_omits_them():
        # An omitted `gates:` means the reusable workflow's default -- the
        # full set. Treating it as "no gates" would silently skip a root.
        (python, _rust) = parse_gate_matrix(CONVENTIONS)
        assert python.gates == ROOT_GATES

    def it_ignores_jobs_that_do_not_call_the_reusable_workflow():
        assert all(e.source != "unrelated" for e in parse_gate_matrix(CONVENTIONS))


def describe_the_real_workflow():
    def it_covers_every_scan_root_ci_declares():
        # The justfile's `test-conventions` recipe restated the matrix by hand
        # and covered 3 of these 8 -- the drift this parser exists to remove.
        with open(".github/workflows/conventions.yml", encoding="utf-8") as handle:
            entries = parse_gate_matrix(handle.read())
        assert sorted(e.source for e in entries) == [
            "internals/checks/src",
            "internals/distcheck/src",
            "packages/python",
            "packages/python/dirsql",
            "packages/rust",
            "packages/ts/napi",
            "packages/ts/src",
            "plugins/dirsql-plugin-embeddings/src",
        ]
