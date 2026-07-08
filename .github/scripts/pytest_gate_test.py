import sys
from unittest import mock

from pytest_gate import NO_TESTS_COLLECTED, interpret, main


def describe_interpret():
    def no_tests_collected_is_green():
        assert interpret(NO_TESTS_COLLECTED) == 0

    def all_passed_is_green():
        assert interpret(0) == 0

    def failure_propagates():
        assert interpret(1) == 1

    def usage_error_propagates():
        assert interpret(4) == 4


def describe_main():
    def runs_pytest_with_forwarded_args_and_returns_interpreted_code():
        runner = mock.Mock(return_value=mock.Mock(returncode=NO_TESTS_COLLECTED))
        rc = main(["pkg/", "-x"], runner=runner)
        runner.assert_called_once_with([sys.executable, "-m", "pytest", "pkg/", "-x"])
        assert rc == 0

    def failing_suite_makes_the_gate_red():
        runner = mock.Mock(return_value=mock.Mock(returncode=1))
        assert main(["pkg/"], runner=runner) == 1
