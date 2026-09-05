"""Colocated unit tests for the pytest-gate orchestration (#494/#495).

Isolation: the finder and the subprocess runner are injected, and the default
bindings are asserted by module and name rather than imported. `interpret` runs
for real -- it is a pure int-in / int-out translation.
"""
import sys
from unittest import mock

from checks.pytest_gate.run import run


def describe_run():
    def runs_pytest_and_returns_interpreted_code_when_tests_exist():
        runner = mock.Mock(return_value=mock.Mock(returncode=5))
        finder = mock.Mock(return_value=["found_test.py"])
        rc = run(["pkg/", "-x"], runner=runner, finder=finder)
        finder.assert_called_once_with(["pkg/"])
        runner.assert_called_once_with([sys.executable, "-m", "pytest", "pkg/", "-x"])
        assert rc == 0

    def failing_suite_makes_the_gate_red():
        runner = mock.Mock(return_value=mock.Mock(returncode=1))
        finder = mock.Mock(return_value=["found_test.py"])
        assert run(["pkg/"], runner=runner, finder=finder) == 1

    def genuine_no_tests_is_green_without_running_pytest(capsys):
        runner = mock.Mock()
        finder = mock.Mock(return_value=[])
        assert run(["-q"], runner=runner, finder=finder) == 0
        runner.assert_not_called()
        assert capsys.readouterr().out == "No *_test.py under ['.'] — nothing to test.\n"

    def it_names_the_scanned_paths_when_they_hold_no_tests(capsys):
        run(["pkg/", "-q"], runner=mock.Mock(), finder=mock.Mock(return_value=[]))
        assert capsys.readouterr().out == "No *_test.py under ['pkg/'] — nothing to test.\n"

    def it_defaults_to_a_real_subprocess_and_the_packages_finder():
        assert [(f.__module__, f.__name__) for f in run.__defaults__] == [
            *[("subprocess", "run"), ("checks.pytest_gate.find_test_files", "find_test_files")]
        ]
