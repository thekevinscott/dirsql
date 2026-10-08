"""Hermetic tests for the speed-of-light harness's own arithmetic.

The runs are fake callables returning canned seconds, so the pairing order,
the per-pair ratio, the median verdict and the startup subtraction are checked
without spawning a process or timing anything.
"""

import pytest

from tests.e2e.speed_of_light import (
    TOLERANCE,
    Paired,
    Startup,
    assert_speed_of_light,
    median_after_first,
    pair_ratios,
    paired,
    summarize,
)

NO_STARTUP = Startup(0.0, 0.0)


def recorder(log, label, seconds):
    def run(*_timeout):
        log.append(label)
        return label, seconds

    return run


def describe_paired():
    def it_discards_one_warm_up_pair_then_runs_the_requested_pairs():
        log = []
        result = paired(
            recorder(log, "N", 1.0), recorder(log, "D", 1.0), NO_STARTUP, pairs=4
        )
        assert log.count("N") == 5
        assert log.count("D") == 5
        assert len(result.native_seconds) == 4
        assert len(result.dirsql_seconds) == 4

    def it_alternates_which_side_runs_first():
        log = []
        paired(recorder(log, "N", 1.0), recorder(log, "D", 1.0), NO_STARTUP, pairs=4)
        assert log == ["N", "D", "N", "D", "D", "N", "N", "D", "D", "N"]

    def it_returns_the_rows_of_each_side():
        result = paired(
            recorder([], "n", 1.0), recorder([], "d", 1.0), NO_STARTUP, pairs=2
        )
        assert (result.native_rows, result.dirsql_rows) == ("n", "d")

    def it_gives_up_after_three_hopeless_pairs_in_a_row():
        result = paired(
            recorder([], "n", 1.0), recorder([], "d", 50.0), NO_STARTUP, pairs=100
        )
        assert len(result.dirsql_seconds) == 3

    def it_keeps_going_when_hopeless_pairs_are_not_consecutive():
        slow = iter([50.0, 1.0] * 50)

        def dirsql(_timeout):
            return "d", next(slow)

        result = paired(recorder([], "n", 1.0), dirsql, NO_STARTUP, pairs=10)
        assert len(result.dirsql_seconds) == 10

    def it_fails_when_a_run_times_out():
        import subprocess

        def native():
            raise subprocess.TimeoutExpired("find", 600)

        with pytest.raises(AssertionError, match="native did not finish within 600s"):
            paired(native, recorder([], "d", 1.0), NO_STARTUP, pairs=1)

    def it_fails_when_dirsql_times_out():
        import subprocess

        def dirsql(timeout):
            raise subprocess.TimeoutExpired("dirsql", timeout)

        with pytest.raises(AssertionError, match="dirsql did not finish within 60s"):
            paired(recorder([], "n", 1.0), dirsql, NO_STARTUP, pairs=1)


def describe_pair_ratios():
    def it_subtracts_each_sides_own_startup():
        ratios = pair_ratios([2.0, 3.0], [1.5, 2.5], Startup(0.5, 1.0))
        assert ratios == [1.0, 1.0]

    def it_rejects_a_native_time_under_its_startup():
        with pytest.raises(AssertionError, match="under its startup"):
            pair_ratios([0.5], [1.0], Startup(0.0, 1.0))


def describe_median_after_first():
    def it_drops_the_first_sample_and_takes_the_median():
        assert median_after_first([100.0, 3.0, 1.0, 2.0]) == 2.0


def describe_summarize():
    def it_reports_min_median_max_and_spread():
        assert summarize([1.0, 2.0, 4.0]) == (1.0, 2.0, 4.0, 1.5)


def with_ratios(ratios):
    return Paired("r", "r", [1.0] * len(ratios), list(ratios))


def describe_assert_speed_of_light():
    def it_passes_on_the_median_despite_outlier_pairs():
        assert_speed_of_light(
            "case", with_ratios([0.9, 1.0, 1.0, 1.05, 9.0]), NO_STARTUP
        )

    def it_fails_when_the_median_is_over_the_bar():
        over = TOLERANCE + 0.2
        with pytest.raises(AssertionError, match="median is over"):
            assert_speed_of_light(
                "case", with_ratios([0.1, over, over, over, over]), NO_STARTUP
            )

    def it_prints_the_distribution_on_failure():
        with pytest.raises(
            AssertionError, match=r"min 2\.000 median 2\.000 max 2\.000"
        ):
            assert_speed_of_light("case", with_ratios([2.0, 2.0, 2.0]), NO_STARTUP)

    def it_subtracts_startup_before_judging():
        assert_speed_of_light(
            "case",
            Paired("r", "r", [2.0, 2.0, 2.0], [2.5, 2.5, 2.5]),
            Startup(0.5, 0.0),
        )
