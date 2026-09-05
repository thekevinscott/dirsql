"""Colocated unit tests for the artifact-completeness orchestration (#790)."""

import inspect

from checks.artifact_completeness.run import run

CONFIG = {
    "package": [
        {"name": "dirsql-npm", "targets": ["linux-x64-gnu", {"triple": "darwin-arm64"}]},
        {"name": "dirsql-rust"},
    ]
}


def full(_d):
    return [("x", [], ["f"])]


def drive(entries, walk=full, echo=None):
    return run(
        "dist",
        "c.toml",
        config=lambda _p: CONFIG,
        entries=lambda _d: entries,
        walk=walk,
        echo=echo or (lambda _line: None),
    )


def describe_run():
    def it_returns_zero_and_says_so_when_every_pair_is_present():
        lines = []
        code = drive(["dirsql-npm-linux-x64-gnu", "dirsql-npm-darwin-arm64"], echo=lines.append)
        assert code == 0
        assert lines == ["ok artifact-completeness: all 2 built (package, target) pairs present"]

    def it_returns_one_and_names_the_pair_and_the_likely_cause():
        lines = []
        code = drive(["dirsql-npm-linux-x64-gnu"], echo=lines.append)
        assert code == 1
        assert lines[0] == (
            "incomplete artifact -- dirsql-npm / darwin-arm64: "
            "no artifact directory matching *darwin-arm64*"
        )
        assert "1 of the built" in lines[1]
        assert "stages where the engine packages from" in lines[1]

    def it_passes_and_says_so_when_the_plan_built_nothing():
        lines = []
        assert drive([], echo=lines.append) == 0
        assert lines == [
            "skip artifact-completeness: dirsql-npm -- the plan built no artifacts for it",
            "ok artifact-completeness: all 0 built (package, target) pairs present",
        ]

    def it_takes_dist_dir_and_config_by_keyword():
        # `*` (not `/`) before the injected seams keeps both nameable.
        assert run(
            dist_dir="dist",
            config_path="c.toml",
            config=lambda _p: {},
            entries=lambda _d: [],
            walk=full,
            echo=lambda _line: None,
        ) == 0


def describe_default_seams():
    def it_defaults_to_the_split_out_config_reader_and_lister():
        params = inspect.signature(run).parameters
        assert params["config"].default.__module__ == "checks.artifact_completeness.read_config"
        assert params["entries"].default.__module__ == "checks.artifact_completeness.subdirectories"
        assert params["echo"].default.__module__ == "checks.artifact_completeness.gate"
