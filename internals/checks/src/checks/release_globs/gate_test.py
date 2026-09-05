"""Colocated unit tests for the release-globs gate (#944).

Isolation: the two readers are injected, so nothing here touches the repo's real
`putitoutthere.toml` or `release-ci.yml`. The readers themselves are exercised
in their own modules' tests.
"""

from checks.release_globs.gate import (
    exclusions,
    glob_problems,
    pull_request_paths,
    read_config,
    read_workflow,
    run,
    unprechecked,
)

CARVED = [
    "packages/rust/!(testing-conventions.toml)",
    "packages/rust/!(changelog.d|migrations.d|e2e-attestations)/**",
]
CLEAN = {"package": [{"name": "dirsql-rust", "globs": CARVED}]}
DIRTY = {"package": [{"name": "dirsql-rust", "globs": ["packages/rust/**"]}]}
NO_PATHS: dict = {}


def describe_collaborators():
    def it_resolves_glob_problems_from_its_own_module():
        assert glob_problems.__module__ == "checks.release_globs.glob_problems"

    def it_resolves_unprechecked_from_the_precheck_module():
        assert unprechecked.__module__ == "checks.release_globs.precheck"

    def it_defaults_the_config_reader_to_the_toml_reader_module():
        assert read_config.__module__ == "checks.release_globs.read_config"

    def it_defaults_the_workflow_reader_to_the_yaml_reader_module():
        assert read_workflow.__module__ == "checks.release_globs.read_workflow"

    def it_resolves_the_path_filter_reader_from_its_own_module():
        assert pull_request_paths.__module__ == "checks.release_globs.pull_request_paths"


def describe_exclusions():
    def it_keeps_only_the_negated_entries():
        assert exclusions(["packages/rust/**", "!packages/rust/changelog.d/**"]) == [
            "!packages/rust/changelog.d/**"
        ]


def drive(config, workflow, lines):
    return run(
        "putitoutthere.toml",
        ".github/workflows/release-ci.yml",
        config=lambda _p: config,
        workflow=lambda _p: workflow,
        echo=lines.append,
    )


def describe_run():
    def it_returns_zero_and_names_both_files_when_they_agree():
        lines: list[str] = []
        assert drive(CLEAN, NO_PATHS, lines) == 0
        assert lines == [
            "ok release-globs: putitoutthere.toml and "
            ".github/workflows/release-ci.yml agree on what ships."
        ]

    def it_returns_one_and_annotates_each_offending_glob():
        lines: list[str] = []
        assert drive(DIRTY, NO_PATHS, lines) == 1
        assert lines[0].startswith("::error::dirsql-rust: publish globs for packages/rust")
        assert lines[1] == (
            "release-globs: 1 problem(s). See "
            "internals/checks/src/checks/release_globs/decide.py for why "
            "leading-`!` negations do not work and what the extglob carve-out form is."
        )

    def it_also_annotates_a_precheck_exclusion_publishing_does_not_share():
        lines: list[str] = []
        workflow = {True: {"pull_request": {"paths": ["packages/rust/**", "!packages/rust/tests/**"]}}}
        assert drive(CLEAN, workflow, lines) == 1
        assert "release-ci.yml excludes" in lines[0]
        assert lines[1].startswith("release-globs: 1 problem(s).")

    def it_accepts_a_config_declaring_no_packages():
        lines: list[str] = []
        assert drive({}, NO_PATHS, lines) == 0

    def it_takes_both_paths_by_keyword():
        # `*` (not `/`) before the injected seams keeps both nameable.
        assert (
            run(
                config_path="c.toml",
                workflow_path="w.yml",
                config=lambda _p: {},
                workflow=lambda _p: {},
                echo=lambda _line: None,
            )
            == 0
        )
