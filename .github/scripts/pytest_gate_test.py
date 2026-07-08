import sys
from unittest import mock

from pytest_gate import NO_TESTS_COLLECTED, find_test_files, interpret, main


def describe_find_test_files():
    def finds_nested_test_files_recursively(tmp_path):
        nested = tmp_path / "a" / "b"
        nested.mkdir(parents=True)
        target = nested / "check_test.py"
        target.write_text("")
        (nested / "check.py").write_text("")
        assert find_test_files([str(tmp_path)]) == [target]

    def returns_empty_when_no_paths():
        assert find_test_files([]) == []


def describe_interpret():
    def no_tests_collected_is_green():
        assert interpret(NO_TESTS_COLLECTED) == 0

    def failure_propagates():
        assert interpret(1) == 1


def describe_main():
    def runs_pytest_and_returns_interpreted_code_when_tests_exist():
        runner = mock.Mock(return_value=mock.Mock(returncode=NO_TESTS_COLLECTED))
        finder = mock.Mock(return_value=["found_test.py"])
        rc = main(["pkg/", "-x"], runner=runner, finder=finder)
        finder.assert_called_once_with(["pkg/"])
        runner.assert_called_once_with([sys.executable, "-m", "pytest", "pkg/", "-x"])
        assert rc == 0

    def failing_suite_makes_the_gate_red():
        runner = mock.Mock(return_value=mock.Mock(returncode=1))
        finder = mock.Mock(return_value=["found_test.py"])
        assert main(["pkg/"], runner=runner, finder=finder) == 1

    def genuine_no_tests_is_green_without_running_pytest():
        runner = mock.Mock()
        finder = mock.Mock(return_value=[])
        assert main(["-q"], runner=runner, finder=finder) == 0
        runner.assert_not_called()
