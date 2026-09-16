"""Colocated unit tests for the preflight matrix runner (#781)."""

from checks.preflight.run import run

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


class MemoryCap:
    """Stand-in for `memory_cap.MemoryCap` -- a value record, faked rather than imported."""

    def __init__(self, prefix, skipped=""):
        self.prefix = prefix
        self.skipped = skipped


CAPPED = MemoryCap(["systemd-run", "--user", "--scope", "-p", "MemoryMax=1M", "-p", "MemorySwapMax=0"])
UNCAPPED = MemoryCap([], "systemd-run is not on PATH")
UNCAPPED_NOTE = "preflight: mutation runs uncapped: systemd-run is not on PATH"


def drive(workflows=None, **kwargs):
    defaults = {
        "runner": lambda _argv, _cwd: 0,
        "exists": has_manifest,
        "e2e_config": lambda _config: {},
        "echo": lambda _line: None,
        "cap": CAPPED,
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
            "==> python-sdk [python] mutation: "
            "systemd-run --user --scope -p MemoryMax=1M -p MemorySwapMax=0 "
            "uv run --with testing-conventions "
            "npx -y testing-conventions@latest unit mutation --language python "
            "--base origin/main dirsql"
        ]

    def it_wraps_a_mutation_pair_in_the_capped_scope_keeping_its_cwd():
        calls = []
        drive(only=["mutation"], runner=lambda argv, cwd: calls.append((argv, cwd)) or 0)
        assert calls == [
            (
                [
                    *CAPPED.prefix,
                    *["uv", "run", "--with", "testing-conventions"],
                    *["npx", "-y", "testing-conventions@latest", "unit", "mutation"],
                    *["--language", "python", "--base", "origin/main", "dirsql"],
                ],
                "packages/python",
            )
        ]

    def it_leaves_every_other_gate_uncapped():
        calls = []
        drive(only=["unit-lint"], runner=lambda argv, _cwd: calls.append(argv) or 0)
        assert [argv[0] for argv in calls] == ["npx"]

    def it_runs_mutation_bare_when_the_cap_is_skipped():
        calls = []
        drive(only=["mutation"], cap=UNCAPPED, runner=lambda argv, _cwd: calls.append(argv) or 0)
        assert [argv[0] for argv in calls] == ["uv"]

    def it_says_once_why_mutation_runs_uncapped_before_the_first_such_pair():
        lines = []
        drive(
            workflows=[CONVENTIONS, CONVENTIONS.replace("python-sdk", "internals-checks")],
            only=["mutation"],
            cap=UNCAPPED,
            echo=lines.append,
        )
        assert lines.count(UNCAPPED_NOTE) == 1
        assert [line.split(": ")[0] for line in lines[:3]] == [
            "preflight",
            "==> python-sdk [python] mutation",
            "==> internals-checks [python] mutation",
        ]

    def it_says_nothing_about_the_cap_when_it_applies():
        lines = []
        drive(only=["mutation"], echo=lines.append)
        assert UNCAPPED_NOTE not in lines

    def it_says_nothing_about_the_cap_when_no_mutation_pair_runs():
        lines = []
        drive(only=["unit-lint"], cap=UNCAPPED, echo=lines.append)
        assert UNCAPPED_NOTE not in lines

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
            cap=CAPPED,
        ) == 0
