"""Colocated unit tests for the preflight drift guards (#782)."""

from checks.preflight.prepare import prepare


class Root:
    """Stand-in for `matrix.Root` -- a value record, faked rather than imported."""

    def __init__(self, job, source, languages, gates, config=None):
        self.job = job
        self.source = source
        self.languages = languages
        self.gates = gates
        self.config = config


PY = Root(
    job="python-sdk",
    source="packages/python/dirsql",
    languages=["python"],
    gates=["unit-lint", "mutation"],
    config="testing-conventions.toml",
)
RUST = Root(job="rust", source="packages/rust", languages=["rust"], gates=["packaging"])


def has_manifest(path: str) -> bool:
    return path == "packages/python/pyproject.toml"


def describe_prepare():
    def it_syncs_and_checks_declared_deps_for_each_python_root():
        assert [(job, step, call.argv) for job, step, call in prepare([PY], has_manifest)] == [
            ("python-sdk", "uv-sync", ["uv", "sync", "--project", "packages/python"]),
            (
                "python-sdk",
                "declared-deps",
                [
                    *["uv", "run", "--project", "internals/checks", "dirsql-checks"],
                    *["declared-deps", "packages/python/dirsql"],
                ],
            ),
        ]

    def it_runs_both_guards_from_the_repo_root():
        assert [call.cwd for _job, _step, call in prepare([PY], has_manifest)] == [".", "."]

    def it_skips_a_root_with_no_python():
        assert prepare([RUST], has_manifest) == []

    def it_keeps_going_past_a_non_python_root_to_the_ones_after_it():
        assert [job for job, _step, _call in prepare([RUST, PY], has_manifest)] == [
            *["python-sdk", "python-sdk"]
        ]
