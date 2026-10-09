"""Hermetic tests for the speed-of-light harness's own arithmetic.

The runs are fake callables returning canned seconds, so the pairing order, the fastest-native bar,
the per-pair ratio, the median verdict and the startup subtraction are checked
without spawning a process or timing anything.
"""

import pytest

from tests.e2e.speed_of_light import (
    FASTEST,
    TOLERANCE,
    Native,
    Natives,
    Paired,
    Startup,
    agreed_rows,
    baseline,
    fd,
    globstar,
    shell_natives,
    assert_speed_of_light,
    fastest_by_median,
    median_after_first,
    pair_ratios,
    paired,
    resolve_natives,
    rg,
    summarize,
)

NO_STARTUP = Startup(0.0, 0.0)


def recorder(log, label, seconds):
    def run(*_timeout):
        log.append(label)
        return label, seconds

    return run


def one(run, name="find"):
    return Natives({name: run}, [])


def describe_paired():
    def it_discards_one_warm_up_pair_then_screens_then_measures():
        log = []
        result = paired(
            one(recorder(log, "N", 1.0)),
            recorder(log, "D", 1.0),
            NO_STARTUP,
            pairs=4,
            screen=2,
        )
        assert log.count("N") == 7
        assert log.count("D") == 7
        assert len(result.native_seconds["find"]) == 4
        assert len(result.dirsql_seconds) == 4
        assert len(result.screen_seconds["find"]) == 2
        assert len(result.screen_dirsql) == 2

    def it_alternates_which_side_runs_first():
        log = []
        paired(
            one(recorder(log, "N", 1.0)),
            recorder(log, "D", 1.0),
            NO_STARTUP,
            pairs=4,
            screen=1,
        )
        assert log == ["N", "D"] * 2 + ["N", "D", "D", "N", "N", "D", "D", "N"]

    def it_screens_every_native_then_measures_only_the_fastest():
        log = []
        natives = Natives(
            {"a": recorder(log, "A", 3.0), "b": recorder(log, "B", 1.0)}, [], FASTEST
        )
        result = paired(natives, recorder(log, "D", 1.0), NO_STARTUP, pairs=2, screen=2)
        assert result.fastest == "b"
        assert set(result.screen_seconds) == {"a", "b"}
        assert set(result.native_seconds) == {"b"}
        assert log.count("A") == 3
        assert log.count("B") == 5

    def it_measures_the_bar_native_beside_the_fastest():
        natives = Natives(
            {"a": recorder([], "A", 3.0), "b": recorder([], "B", 1.0)}, []
        )
        result = paired(natives, recorder([], "D", 1.0), NO_STARTUP, pairs=2, screen=1)
        assert (result.bar, result.fastest) == ("a", "b")
        assert set(result.native_seconds) == {"a", "b"}

    def it_pins_the_bar_to_the_fastest_when_asked():
        natives = Natives(
            {"a": recorder([], "A", 3.0), "b": recorder([], "B", 1.0)}, [], FASTEST
        )
        result = paired(natives, recorder([], "D", 1.0), NO_STARTUP, pairs=2, screen=1)
        assert result.bar == "b"

    def it_pins_the_bar_to_a_named_native():
        natives = Natives(
            {"a": recorder([], "A", 3.0), "b": recorder([], "B", 1.0)}, [], "b"
        )
        result = paired(natives, recorder([], "D", 1.0), NO_STARTUP, pairs=2, screen=1)
        assert result.bar == "b"

    def it_times_every_native_in_every_screening_pair():
        log = []
        natives = Natives(
            {"a": recorder(log, "A", 1.0), "b": recorder(log, "B", 2.0)}, []
        )
        result = paired(natives, recorder(log, "D", 1.0), NO_STARTUP, pairs=1, screen=2)
        assert log[3:10] == ["A", "B", "D", "D", "B", "A", "A"]
        assert result.screen_seconds == {"a": [1.0, 1.0], "b": [2.0, 2.0]}

    def it_returns_the_rows_of_each_side():
        result = paired(
            one(recorder([], "n", 1.0)),
            recorder([], "d", 1.0),
            NO_STARTUP,
            pairs=2,
            screen=1,
        )
        assert (result.native_rows, result.dirsql_rows) == ({"find": "n"}, "d")

    def it_carries_the_skipped_natives_through():
        natives = Natives({"find": recorder([], "n", 1.0)}, ["fd"])
        result = paired(natives, recorder([], "d", 1.0), NO_STARTUP, pairs=1, screen=1)
        assert result.skipped == ["fd"]

    def it_gives_up_after_three_hopeless_pairs_in_a_row():
        result = paired(
            one(recorder([], "n", 1.0)), recorder([], "d", 50.0), NO_STARTUP, pairs=100
        )
        assert len(result.dirsql_seconds) == 3

    def it_judges_hopelessness_against_the_fastest_native():
        natives = Natives(
            {"slow": recorder([], "s", 40.0), "quick": recorder([], "q", 1.0)}, []
        )
        result = paired(natives, recorder([], "d", 50.0), NO_STARTUP, pairs=100, screen=1)
        assert len(result.dirsql_seconds) == 3

    def it_keeps_going_when_hopeless_pairs_are_not_consecutive():
        slow = iter([50.0, 1.0] * 50)

        def dirsql(_timeout):
            return "d", next(slow)

        result = paired(one(recorder([], "n", 1.0)), dirsql, NO_STARTUP, pairs=10, screen=1)
        assert len(result.dirsql_seconds) == 10

    def it_fails_when_a_run_times_out():
        import subprocess

        def native():
            raise subprocess.TimeoutExpired("find", 600)

        with pytest.raises(AssertionError, match="find did not finish within 600s"):
            paired(one(native), recorder([], "d", 1.0), NO_STARTUP, pairs=1, screen=1)

    def it_fails_when_dirsql_times_out():
        import subprocess

        def dirsql(timeout):
            raise subprocess.TimeoutExpired("dirsql", timeout)

        with pytest.raises(AssertionError, match="dirsql did not finish within 60s"):
            paired(one(recorder([], "n", 1.0)), dirsql, NO_STARTUP, pairs=1, screen=1)


def installed(*names):
    return lambda binary: f"/bin/{binary}" if binary in names else None


def describe_resolve_natives():
    def it_keeps_the_natives_whose_tool_is_installed():
        specs = [Native("find", ("find",), "{bin} .")]
        runnable, skipped = resolve_natives(specs, installed("find"))
        assert runnable == [("find", "find .")]
        assert skipped == []

    def it_falls_back_to_the_next_name_for_a_tool():
        specs = [Native("fd", ("fd", "fdfind"), "{bin} -t f")]
        runnable, _ = resolve_natives(specs, installed("fdfind"))
        assert runnable == [("fd", "fdfind -t f")]

    def it_reports_a_missing_tool_as_skipped():
        specs = [
            Native("find", ("find",), "{bin} ."),
            Native("rg", ("rg",), "{bin} --files"),
        ]
        runnable, skipped = resolve_natives(specs, installed("find"))
        assert [name for name, _ in runnable] == ["find"]
        assert skipped == ["rg"]

    def it_fails_when_no_native_is_installed():
        with pytest.raises(AssertionError, match="no native installed"):
            resolve_natives([Native("rg", ("rg",), "{bin}")], installed())


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


def with_ratios(ratios, skipped=()):
    n = len(ratios)
    return Paired(
        {"find": "r"},
        "r",
        {"find": [1.0] * n},
        list(ratios),
        list(skipped),
        {"find": [1.0] * n},
        list(ratios),
        "find",
        "find",
    )


def describe_fastest_by_median():
    def it_picks_the_lowest_median_time():
        assert fastest_by_median({"a": [3.0, 3.0, 3.0], "b": [9.0, 1.0, 1.0]}) == "b"


def describe_agreed_rows():
    def it_returns_the_rows_all_natives_share():
        result = Paired({"a": [1], "b": [1]}, [1], {}, [], [])
        assert agreed_rows(result) == [1]

    def it_names_the_native_that_disagrees():
        result = Paired({"a": [1], "b": [2]}, [1], {}, [], [])
        with pytest.raises(AssertionError, match="b disagrees with a"):
            agreed_rows(result)


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
        result = with_ratios([2.5] * 3)
        result.native_seconds = {"find": [2.0] * 3}
        result.screen_seconds = {"find": [2.0] * 3}
        assert_speed_of_light("case", result, Startup(0.5, 0.0))

    def it_holds_dirsql_to_the_bar_not_the_fastest():
        result = Paired(
            {"slow": "r", "quick": "r"},
            "r",
            {"slow": [10.0] * 3, "quick": [1.0] * 3},
            [2.0] * 3,
            [],
            {"slow": [10.0] * 3, "quick": [1.0] * 3},
            [2.0] * 3,
            "quick",
            "slow",
        )
        assert_speed_of_light("case", result, NO_STARTUP)

    def it_fails_when_dirsql_is_over_the_bar_native():
        result = Paired(
            {"slow": "r", "quick": "r"},
            "r",
            {"slow": [1.0] * 3, "quick": [1.0] * 3},
            [2.0] * 3,
            [],
            {"slow": [1.0] * 3, "quick": [1.0] * 3},
            [2.0] * 3,
            "quick",
            "quick",
        )
        with pytest.raises(AssertionError, match=r"bar native quick.*over"):
            assert_speed_of_light("case", result, NO_STARTUP)

    def it_prints_the_fastest_ratio_beside_the_bar(capsys):
        result = Paired(
            {"find": "r", "rg": "r"},
            "r",
            {"find": [2.0] * 3, "rg": [1.0] * 3},
            [2.0] * 3,
            [],
            {"find": [2.0] * 3, "rg": [1.0] * 3},
            [2.0] * 3,
            "rg",
            "find",
        )
        assert_speed_of_light("case", result, NO_STARTUP)
        out = capsys.readouterr().out
        assert "bar case vs find" in out
        assert "fastest case vs rg: 3 pairs, ratio min 2.000" in out

    def it_prints_every_screened_native_and_the_skipped_ones(capsys):
        result = Paired(
            {"find": "r", "rg": "r"},
            "r",
            {"find": [1.0] * 3},
            [1.0] * 3,
            ["fd"],
            {"find": [1.0] * 5, "rg": [2.0] * 5},
            [1.0] * 5,
            "find",
            "find",
        )
        assert_speed_of_light("case", result, NO_STARTUP)
        out = capsys.readouterr().out
        assert "case screen vs find: 5 pairs, ratio min 1.000" in out
        assert "case screen vs rg: 5 pairs, ratio min 0.500" in out
        assert "SKIPPED fd" in out


def describe_native_specs():
    def it_lets_fd_go_by_either_name():
        assert fd("--glob x").needs == ("fd", "fdfind")

    def it_has_rg_list_files_without_honoring_ignore_files():
        assert "--files --no-ignore" in rg("-g x").script

    def it_strips_the_dot_slash_from_globstar_output_when_the_pattern_has_one():
        assert globstar("./**/*.md").script.endswith("| cut -c3-")
        assert globstar(".cache/**/*.md").script.endswith("| cat")


def describe_baseline():
    def it_is_the_first_native():
        first, second = object(), object()
        assert baseline(Natives({"a": first, "b": second}, [])) is first


def describe_shell_natives():
    def it_carries_the_bar_through(tmp_path):
        specs = [Native("true", ("true",), "{bin}")]
        assert shell_natives(tmp_path, specs, bar=FASTEST).bar == FASTEST
        assert shell_natives(tmp_path, specs).bar == ""
