"""Colocated unit tests for reading a workflow's pull-request path filter (#944)."""

from checks.release_globs.pull_request_paths import pull_request_paths


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
