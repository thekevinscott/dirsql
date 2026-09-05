"""Colocated unit tests for gate-matrix source resolution (#781)."""

import inspect
import os
from unittest import mock

import pytest

from checks.preflight.sources import sources

CONVENTIONS = "jobs: {}"


class NoGateMatrix(Exception):
    """Stand-in for `matrix.NoGateMatrix` -- raised by the mocked collaborators."""


def describe_sources():
    def it_reads_each_named_workflow_in_the_order_given():
        with mock.patch(
            "checks.preflight.sources.named", side_effect=lambda path, _read: f"text of {path}"
        ):
            assert sources(["b.yml", "a.yml"], read=lambda path: path) == [
                ("b.yml", "text of b.yml"),
                ("a.yml", "text of a.yml"),
            ]

    def it_hands_each_named_workflow_the_reader_it_was_given():
        read = lambda path: path  # noqa: E731
        with mock.patch("checks.preflight.sources.named") as named:
            sources(["wf.yml"], read=read)

        assert named.call_args.args == ("wf.yml", read)

    def it_surfaces_the_failure_when_a_named_workflow_declares_no_caller():
        with mock.patch("checks.preflight.sources.named", side_effect=NoGateMatrix("no job")):
            with pytest.raises(NoGateMatrix):
                sources(["docs.yml"], read=lambda _path: CONVENTIONS)

    def it_discovers_the_callers_when_no_workflow_is_named():
        listdir = lambda _directory: ["ci.yml"]  # noqa: E731
        read = lambda path: path  # noqa: E731
        with mock.patch(
            "checks.preflight.sources.discovered", return_value=[("wf/ci.yml", CONVENTIONS)]
        ) as discovered:
            assert sources((), directory="wf", listdir=listdir, read=read) == [
                ("wf/ci.yml", CONVENTIONS)
            ]

        assert discovered.call_args.args == ("wf", listdir, read)

    def it_takes_its_seams_by_keyword_only():
        # `*` (not `/`) before the injected seams: a third positional argument
        # would otherwise land silently on `listdir`.
        with pytest.raises(TypeError):
            sources((), "wf", lambda _directory: ["ci.yml"], lambda path: path)

    def it_discovers_from_the_workflows_directory_by_default():
        # The default is the whole point: #834 deleted the workflow the command
        # used to name, and nothing pinned where it looks instead.
        with mock.patch("checks.preflight.sources.discovered", return_value=[]) as discovered:
            sources(())

        assert discovered.call_args.args[0] == ".github/workflows"


def describe_default_seams():
    def it_lists_with_the_stdlib_and_reads_with_the_split_out_reader():
        params = inspect.signature(sources).parameters

        assert params["listdir"].default is os.listdir
        assert params["read"].default.__module__ == "checks.preflight.read_text"
