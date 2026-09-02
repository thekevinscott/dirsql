"""Colocated unit tests for the preflight runner (#781)."""

from unittest import mock

from checks.preflight.gate import (
    Invocation,
    prepare,
    default_runner,
    e2e_flags,
    invocation,
    package_root,
    read_e2e,
    run,
)


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
CONVENTIONS = """
jobs:
  python-sdk:
    uses: x/.github/workflows/testing-conventions.yml@v0
    with:
      languages: '["python"]'
      source: packages/python/dirsql
      gates: '["unit-lint", "mutation"]'
"""
# `packaging` first, so a `break` in place of the skip's `continue` would drop
# the pair after it.
ARTIFACT_FIRST = CONVENTIONS.replace('"unit-lint", "mutation"', '"packaging", "unit-lint"')


def has_manifest(path: str) -> bool:
    return path == "packages/python/pyproject.toml"


def call(root, language, gate, e2e=None):
    return invocation(root, language, gate, "origin/main", has_manifest, e2e or {})


def describe_package_root():
    def it_walks_up_to_the_nearest_manifest():
        assert package_root("packages/python/dirsql", has_manifest) == "packages/python"

    def it_returns_the_source_itself_when_it_holds_the_manifest():
        assert package_root("packages/python", has_manifest) == "packages/python"

    def it_falls_back_to_the_repo_root_when_no_ancestor_has_one():
        assert package_root("packages/rust", lambda _path: False) == "."


def describe_e2e_flags():
    def it_maps_extra_scope_and_exclude_onto_repeatable_flags():
        assert e2e_flags({"extra_scope": ["a", "b"], "exclude": ["a/cli"]}) == [
            *["--extra-scope", "a", "--extra-scope", "b"],
            *["--exclude", "a/cli"],
        ]

    def it_returns_nothing_for_an_absent_table():
        assert e2e_flags({}) == []


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


def describe_read_e2e():
    def it_returns_the_e2e_table_of_the_roots_config():
        with mock.patch("checks.preflight.gate.os.path.exists", return_value=True):
            with mock.patch("checks.preflight.gate.open", mock.mock_open(read_data=b"")):
                with mock.patch(
                    "checks.preflight.gate.tomllib.load",
                    return_value={"e2e": {"extra_scope": ["x"]}},
                ):
                    assert read_e2e("c.toml") == {"extra_scope": ["x"]}

    def it_returns_empty_for_a_root_with_no_config():
        assert read_e2e(None) == {}

    def it_returns_empty_when_the_config_is_absent_from_disk():
        with mock.patch("checks.preflight.gate.os.path.exists", return_value=False):
            assert read_e2e("gone.toml") == {}


def describe_default_runner():
    def it_returns_the_subprocess_return_code():
        with mock.patch(
            "checks.preflight.gate.subprocess.run",
            return_value=mock.Mock(returncode=3),
        ) as subprocess_run:
            assert default_runner(["x"], "dir") == 3
        subprocess_run.assert_called_once_with(["x"], cwd="dir", check=False)


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

    def it_skips_a_root_with_no_python():
        assert prepare([RUST], has_manifest) == []

    def it_keeps_going_past_a_non_python_root_to_the_ones_after_it():
        assert [job for job, _step, _call in prepare([RUST, PY], has_manifest)] == [
            *["python-sdk", "python-sdk"]
        ]


def drive(workflows=None, **kwargs):
    defaults = {
        "runner": lambda _argv, _cwd: 0,
        "exists": has_manifest,
        "e2e_config": lambda _config: {},
        "echo": lambda _line: None,
    }
    return run(workflows or [CONVENTIONS], "origin/main", **{**defaults, **kwargs})


def describe_run():
    def it_runs_the_drift_guards_first_then_every_pair():
        calls = []
        assert drive(runner=lambda argv, cwd: calls.append((argv, cwd)) or 0) == 0
        assert [argv[:2] for argv, _cwd in calls] == [
            *[["uv", "sync"], ["uv", "run"]],
            *[["npx", "-y"], ["uv", "run"]],
        ]
        assert [cwd for _argv, cwd in calls] == [".", ".", ".", "packages/python"]

    def it_returns_one_and_names_each_failing_pair():
        lines = []
        code = drive(runner=lambda argv, _cwd: 1 if "mutation" in argv else 0, echo=lines.append)
        assert code == 1
        assert "FAIL python-sdk [python] mutation" in lines
        assert "preflight: 1 failing pair(s), 0 skipped" in lines

    def it_counts_any_non_zero_exit_as_a_failure_including_a_negative_one():
        # A signal-killed gate reports a negative code; `> 0` would call it a pass.
        assert drive(runner=lambda _argv, _cwd: -1) == 1

    def it_skips_an_artifact_gate_without_failing_and_says_so():
        lines = []
        code = drive(
            workflows=[ARTIFACT_FIRST],
            runner=lambda _argv, _cwd: 0,
            echo=lines.append,
        )
        assert code == 0
        assert (
            "SKIP python-sdk [python] packaging: "
            "needs a built artifact, which CI builds from the manifest"
        ) in lines
        assert "preflight: 0 failing pair(s), 1 skipped" in lines

    def it_keeps_going_past_a_skipped_gate_to_the_pairs_after_it():
        lines = []
        drive(
            workflows=[ARTIFACT_FIRST],
            only=["packaging", "unit-lint"],
            echo=lines.append,
        )
        assert [line[:4] for line in lines[:2]] == ["SKIP", "==> "]

    def it_keeps_going_past_a_filtered_out_drift_guard_to_the_next_one():
        # `uv-sync` comes first, so a `break` on the filter would drop declared-deps.
        lines = []
        drive(only=["declared-deps"], echo=lines.append)
        assert [line.split(": ")[0] for line in lines if line.startswith("==>")] == [
            "==> python-sdk [python] declared-deps"
        ]

    def it_keeps_going_past_a_filtered_out_gate_to_the_pairs_after_it():
        lines = []
        drive(only=["mutation"], echo=lines.append)
        assert len([line for line in lines if line.startswith("==>")]) == 1

    def it_echoes_the_argv_it_is_about_to_run():
        lines = []
        drive(only=["unit-lint"], echo=lines.append)
        assert lines[0] == (
            "==> python-sdk [python] unit-lint: npx -y testing-conventions@latest unit lint "
            "--language python packages/python/dirsql"
        )

    def it_runs_only_the_gates_named_by_the_filter():
        lines = []
        drive(only=["mutation"], runner=lambda _argv, _cwd: 0, echo=lines.append)
        assert [line for line in lines if line.startswith("==>")] == [
            "==> python-sdk [python] mutation: uv run --with testing-conventions "
            "npx -y testing-conventions@latest unit mutation --language python "
            "--base origin/main dirsql"
        ]

    def it_prints_every_pair_without_running_any_when_dry_run():
        calls, lines = [], []
        code = drive(
            dry_run=True,
            runner=lambda argv, cwd: calls.append((argv, cwd)) or 1,
            echo=lines.append,
        )
        assert (calls, code) == ([], 0)
        assert len([line for line in lines if line.startswith("==>")]) == 4

    def it_runs_the_pairs_of_every_workflow_it_is_given():
        # Post-#834 the callers live in six workflows, so a matrix built from the
        # first one alone would be a green run covering one lane (#973).
        lines = []
        drive(
            workflows=[CONVENTIONS, CONVENTIONS.replace("python-sdk", "internals-checks")],
            only=["unit-lint"],
            echo=lines.append,
        )
        assert [line.split(": ")[0] for line in lines if line.startswith("==>")] == [
            "==> python-sdk [python] unit-lint",
            "==> internals-checks [python] unit-lint",
        ]

    def it_derives_no_pair_from_a_workflow_with_no_callers():
        lines = []
        assert drive(workflows=["jobs: {}"], echo=lines.append) == 0
        assert lines == ["preflight: 0 failing pair(s), 0 skipped"]

    def it_takes_the_workflows_and_base_by_keyword():
        # `*` (not `/`) before the injected seams: the two leading parameters must
        # stay nameable, since every caller passes the workflow texts by name.
        assert run(
            workflows=[CONVENTIONS],
            base="origin/main",
            runner=lambda _argv, _cwd: 0,
            exists=has_manifest,
            e2e_config=lambda _config: {},
            echo=lambda _line: None,
        ) == 0
