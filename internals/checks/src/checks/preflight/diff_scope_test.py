"""Colocated unit tests for the diff-scope warning."""

from checks.preflight.diff_scope import diff_scope_warning


class Tree:
    """Stand-in for `tree.Tree` -- a value record, faked rather than imported."""

    def __init__(self, dirty, committed, base="origin/main"):
        self.base = base
        self.dirty = dirty
        self.committed = committed


SCOPED = ["colocated-test", "unit-coverage", "mutation", "e2e-verify"]
GATES = ["unit-lint", *SCOPED, "one-function-per-file"]


def describe_diff_scope_warning():
    def it_says_nothing_for_a_clean_tree():
        assert diff_scope_warning(Tree(dirty=False, committed=True), GATES) == []

    def it_says_nothing_when_no_diff_scoped_gate_is_running():
        assert diff_scope_warning(Tree(dirty=True, committed=False), ["unit-lint"]) == []

    def it_names_the_gates_that_will_examine_nothing_when_nothing_is_committed():
        assert diff_scope_warning(Tree(dirty=True, committed=False), GATES) == [
            "preflight: the working tree is dirty but nothing is committed against origin/main.",
            "preflight: these gates read the committed range, so they will examine nothing:",
            "preflight:   colocated-test, unit-coverage, mutation, e2e-verify",
            "preflight: commit first, then re-run.",
        ]

    def it_names_the_base_it_was_given():
        lines = diff_scope_warning(Tree(dirty=True, committed=False, base="origin/dev"), GATES)
        assert lines[0].endswith("nothing is committed against origin/dev.")

    def it_warns_that_uncommitted_edits_go_unseen_when_some_of_the_diff_is_committed():
        assert diff_scope_warning(Tree(dirty=True, committed=True), GATES) == [
            "preflight: the working tree is dirty, so these gates measure the committed",
            "preflight: range only and will not see the uncommitted edits:",
            "preflight:   colocated-test, unit-coverage, mutation, e2e-verify",
            "preflight: commit first, then re-run.",
        ]

    def it_names_each_gate_once_in_matrix_order():
        lines = diff_scope_warning(Tree(dirty=True, committed=False), ["mutation", "mutation"])
        assert lines[2] == "preflight:   mutation"
