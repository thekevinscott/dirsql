"""Colocated unit tests for the preflight argv builder (#781)."""

from checks.preflight.invocation import Invocation, invocation


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


def call(root, language, gate, e2e=None):
    return invocation(root, language, gate, "origin/main", has_manifest, e2e or {})


def describe_invocation():
    def it_runs_an_ordinary_gate_through_the_npm_cli_from_the_repo_root():
        assert call(PY, "python", "unit-lint") == Invocation(
            [
                *["npx", "-y", "testing-conventions@latest", "unit", "lint"],
                *["--language", "python"],
                *["--config", "testing-conventions.toml", "packages/python/dirsql"],
            ],
            ".",
        )

    def it_takes_the_ordinary_path_for_a_gate_name_sorting_below_e2e_verify():
        # Both paths start with the same CLI prefix, so `--scope` is what tells
        # them apart -- an `<=` here would route colocated-test into e2e's branch.
        assert call(PY, "python", "colocated-test").argv == [
            *["npx", "-y", "testing-conventions@latest", "unit", "colocated-test"],
            *["--language", "python", "--base", "origin/main"],
            *["--config", "testing-conventions.toml", "packages/python/dirsql"],
        ]

    def it_omits_base_for_a_whole_tree_gate_that_does_not_accept_it():
        assert "--base" not in call(PY, "python", "unit-lint").argv

    def it_passes_base_to_a_diff_scoped_gate():
        assert "--base" in call(PY, "python", "colocated-test").argv

    def it_omits_base_for_rust_colocated_test_which_the_cli_rejects():
        # Rust units are inline, so the co-change variant has no sibling test
        # that could go stale and the CLI errors on `--base --language rust`.
        assert "--base" not in call(RUST, "rust", "colocated-test").argv

    def it_still_passes_base_for_rust_mutation():
        assert "--base" in call(RUST, "rust", "mutation").argv

    def it_omits_config_for_a_root_that_declares_none():
        assert "--config" not in call(RUST, "rust", "unit-lint").argv

    def it_targets_the_package_root_for_e2e_verify_scoped_to_the_source():
        assert call(PY, "python", "e2e-verify", {"extra_scope": ["packages/rust/src"]}) == Invocation(
            [
                *["npx", "-y", "testing-conventions@latest", "e2e", "verify"],
                *["--base", "origin/main"],
                *["--scope", "packages/python/dirsql"],
                *["--extra-scope", "packages/rust/src", "packages/python"],
            ],
            ".",
        )

    def it_omits_language_for_e2e_verify_which_does_not_accept_it():
        assert "--language" not in call(PY, "python", "e2e-verify").argv

    def it_runs_python_mutation_through_the_packages_own_venv():
        mutation = call(PY, "python", "mutation")
        assert mutation.cwd == "packages/python"
        assert mutation.argv[:7] == [
            *["uv", "run", "--with", "testing-conventions"],
            *["npx", "-y", "testing-conventions@latest"],
        ]

    def it_rewrites_the_config_and_source_paths_relative_to_that_cwd():
        assert call(PY, "python", "mutation").argv[-3:] == [
            *["--config", "../../testing-conventions.toml", "dirsql"]
        ]

    def it_passes_dot_as_the_source_when_the_package_root_is_the_source():
        root = Root(job="p", source="packages/python", languages=["python"], gates=["mutation"])
        assert call(root, "python", "mutation").argv[-1] == "."

    def it_leaves_the_config_alone_when_a_mutation_root_declares_none():
        root = Root(job="p", source="packages/python", languages=["python"], gates=["mutation"])
        assert "--config" not in call(root, "python", "mutation").argv

    def it_runs_python_unit_coverage_through_that_venv_too():
        assert call(PY, "python", "unit-coverage").cwd == "packages/python"

    def it_runs_a_typescript_suite_gate_through_npx_from_the_package_root():
        root = Root(job="ts", source="packages/ts/src", languages=["typescript"], gates=["mutation"])
        mutation = call(root, "typescript", "mutation")
        assert mutation.argv[:3] == ["npx", "-y", "testing-conventions@latest"]
        assert (mutation.cwd, mutation.argv[-1]) == (".", "packages/ts/src")

    def it_keeps_a_rust_mutation_gate_on_the_default_cli():
        assert call(RUST, "rust", "mutation").cwd == "."
