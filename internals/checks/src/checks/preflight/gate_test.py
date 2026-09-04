"""Colocated unit tests for the preflight runner (#781)."""

from unittest import mock

from checks.preflight.gate import default_runner, read_e2e, run

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
