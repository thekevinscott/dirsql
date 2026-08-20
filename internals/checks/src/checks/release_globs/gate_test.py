"""Colocated unit tests for the release-globs gate (#944).

Isolation: the two readers are injected, so nothing here touches the repo's real
`putitoutthere.toml` or `release-ci.yml`. The readers themselves are exercised
against scratch files.
"""

from checks.release_globs.gate import (
    exclusions,
    pull_request_paths,
    read_config,
    read_workflow,
    run,
)

CARVED = [
    "packages/rust/!(testing-conventions.toml)",
    "packages/rust/!(changelog.d|migrations.d|e2e-attestations)/**",
]
CLEAN = {"package": [{"name": "dirsql-rust", "globs": CARVED}]}
DIRTY = {"package": [{"name": "dirsql-rust", "globs": ["packages/rust/**"]}]}
NO_PATHS: dict = {}


def describe_read_config():
    def it_parses_a_toml_release_config(tmp_path):
        path = tmp_path / "putitoutthere.toml"
        path.write_text('[[package]]\nname = "x"\nglobs = ["a/**"]\n')
        assert read_config(str(path)) == {"package": [{"name": "x", "globs": ["a/**"]}]}


def describe_read_workflow():
    def it_parses_a_workflow_whose_on_key_yaml_resolves_to_a_boolean(tmp_path):
        path = tmp_path / "w.yml"
        path.write_text("on:\n  pull_request:\n    paths:\n      - 'a/**'\n")
        assert read_workflow(str(path))[True] == {"pull_request": {"paths": ["a/**"]}}


def describe_pull_request_paths():
    def it_reads_the_filter_off_the_boolean_on_key():
        assert pull_request_paths({True: {"pull_request": {"paths": ["a/**"]}}}) == ["a/**"]

    def it_reads_the_filter_off_a_quoted_string_on_key():
        assert pull_request_paths({"on": {"pull_request": {"paths": ["a/**"]}}}) == ["a/**"]

    def it_is_empty_when_the_workflow_declares_no_triggers():
        assert pull_request_paths({}) == []

    def it_is_empty_when_the_on_block_is_null():
        assert pull_request_paths({True: None}) == []

    def it_is_empty_when_the_workflow_has_no_pull_request_trigger():
        assert pull_request_paths({True: {"push": {"branches": ["main"]}}}) == []

    def it_is_empty_when_the_pull_request_trigger_is_null():
        assert pull_request_paths({True: {"pull_request": None}}) == []

    def it_is_empty_when_the_pull_request_trigger_filters_no_paths():
        assert pull_request_paths({True: {"pull_request": {"branches": ["main"]}}}) == []


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
