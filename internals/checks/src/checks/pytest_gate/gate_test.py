from checks.pytest_gate.gate import NO_TESTS_COLLECTED, interpret


def describe_interpret():
    def no_tests_collected_is_green():
        assert interpret(NO_TESTS_COLLECTED) == 0

    def failure_propagates():
        assert interpret(1) == 1
