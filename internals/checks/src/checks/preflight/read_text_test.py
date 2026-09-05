"""Colocated unit tests for the workflow reader (#781)."""

from unittest import mock

from checks.preflight.read_text import read_text


def describe_read_text():
    def it_reads_a_workflow_as_utf8():
        with mock.patch(
            "checks.preflight.read_text.open", mock.mock_open(read_data="jobs: {}\n")
        ) as opened:
            assert read_text("wf.yml") == "jobs: {}\n"
        opened.assert_called_once_with("wf.yml", encoding="utf-8")
