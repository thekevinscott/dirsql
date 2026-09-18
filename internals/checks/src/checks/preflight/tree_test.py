"""Colocated unit tests for the working-tree probe behind the diff-scope warning."""

from unittest import mock

from checks.preflight.tree import Tree, detect_tree


def probe(status=" M a.py\n", diff="a.py\n", code=0):
    def fake_run(argv, **_kwargs):
        asked_status = argv[1] == "status"
        return mock.Mock(stdout=status if asked_status else diff, returncode=0 if asked_status else code)

    with mock.patch("checks.preflight.tree.subprocess.run", side_effect=fake_run) as run:
        return detect_tree("origin/main"), run


def describe_detect_tree():
    def it_reports_a_dirty_tree_with_a_committed_diff():
        tree, _run = probe()
        assert tree == Tree(base="origin/main", dirty=True, committed=True)

    def it_reports_a_clean_tree_when_status_is_empty():
        tree, _run = probe(status="")
        assert tree.dirty is False

    def it_reports_nothing_committed_when_the_range_diff_is_empty():
        tree, _run = probe(diff="   \n")
        assert tree.committed is False

    def it_reports_committed_when_the_range_cannot_be_resolved():
        tree, _run = probe(diff="", code=128)
        assert tree.committed is True

    def it_asks_git_for_the_porcelain_status_and_the_three_dot_range():
        _tree, run = probe()
        assert [call.args[0] for call in run.call_args_list] == [
            ["git", "status", "--porcelain"],
            ["git", "diff", "--name-only", "origin/main...HEAD"],
        ]
