"""Hermetic tests for the performance suite's report and native selection.

The runs are fake callables returning canned seconds, so every native's
ratio, the machine-readable record, the rendered markdown and the suite-mode
switch are checked without spawning a process or timing anything.
"""

import json

from tests.e2e.bench_report import (
    append_record,
    build_report,
    case_record,
    read_records,
    render_markdown,
)
from tests.e2e.speed_of_light import (
    Natives,
    Paired,
    Startup,
    assert_speed_of_light,
    paired,
)

NO_STARTUP = Startup(0.0, 0.0)


def result(natives, dirsql, skipped=()):
    fastest = min(natives, key=lambda n: sorted(natives[n])[len(natives[n]) // 2])
    return Paired(
        {n: "r" for n in natives},
        "r",
        natives,
        dirsql,
        list(skipped),
        {},
        [],
        fastest,
        fastest,
    )


def canned(seconds):
    def run(*_timeout):
        return "r", seconds

    return run


META = {
    "commit": "abc123",
    "date": "2026-10-09T00:00:00Z",
    "machine": {"platform": "Linux", "cpus": 8},
    "versions": {"dirsql": "0.4.0", "find": "find 4.9"},
    "pairs": 3,
}


def describe_case_record():
    def it_gives_every_native_its_own_ratio_and_spread():
        rec = case_record(
            "./**/*.md",
            result({"find": [2.0, 2.0, 2.0], "rg": [1.0, 1.0, 1.0]}, [2.0, 3.0, 4.0]),
            NO_STARTUP,
        )
        assert rec["name"] == "./**/*.md"
        assert rec["natives"]["find"]["ratio"] == {
            "min": 1.0,
            "median": 1.5,
            "max": 2.0,
            "spread": 2 / 3,
        }
        assert rec["natives"]["rg"]["ratio"]["median"] == 3.0
        assert rec["natives"]["rg"]["native_median_s"] == 1.0

    def it_names_the_fastest_native_and_its_ratio():
        rec = case_record(
            "c", result({"find": [2.0] * 3, "rg": [1.0] * 3}, [3.0] * 3), NO_STARTUP
        )
        assert rec["fastest"] == "rg"
        assert rec["ratio_vs_fastest"] == 3.0
        assert rec["dirsql_median_s"] == 3.0

    def it_subtracts_each_sides_startup():
        rec = case_record("c", result({"find": [2.0] * 3}, [1.5] * 3), Startup(0.5, 1.0))
        assert rec["ratio_vs_fastest"] == 1.0
        assert rec["startup"] == {"dirsql": 0.5, "native": 1.0}

    def it_carries_the_skipped_natives():
        rec = case_record("c", result({"find": [1.0] * 3}, [1.0] * 3, ["fd"]), NO_STARTUP)
        assert rec["skipped"] == ["fd"]


def describe_build_report():
    def it_carries_the_metadata_and_orders_cases_by_name():
        a = case_record("b-case", result({"find": [1.0] * 3}, [1.0] * 3), NO_STARTUP)
        b = case_record("a-case", result({"find": [1.0] * 3}, [1.0] * 3), NO_STARTUP)
        report = build_report([a, b], META)
        assert [c["name"] for c in report["cases"]] == ["a-case", "b-case"]
        for key, value in META.items():
            assert report[key] == value
        json.dumps(report)


def describe_render_markdown():
    def report():
        slow = case_record(
            "slow-case",
            result({"find": [2.0] * 3, "bash-globstar": [1.0] * 3}, [3.0] * 3),
            NO_STARTUP,
        )
        fast = case_record(
            "fast-case", result({"find": [2.0] * 3}, [1.0] * 3, ["fd"]), NO_STARTUP
        )
        return build_report([slow, fast], META)

    def it_states_commit_date_machine_and_versions():
        md = render_markdown(report())
        assert "abc123" in md
        assert "2026-10-09T00:00:00Z" in md
        assert "Linux" in md
        assert "find 4.9" in md
        assert "3 paired runs" in md

    def it_lists_each_case_with_its_ratio_to_the_fastest_native():
        md = render_markdown(report())
        assert "| slow-case | bash-globstar | 3.00x |" in md
        assert "| fast-case | find | 0.50x |" in md

    def it_flags_cases_above_the_fastest_native():
        md = render_markdown(report())
        assert "slow-case | bash-globstar | 3.00x | 0% | over" in md
        assert "fast-case | find | 0.50x | 0% | ok" in md

    def it_details_every_native_and_the_skipped_ones():
        md = render_markdown(report())
        assert "| bash-globstar | 1.000 | 3.00x |" in md
        assert "| find | 2.000 | 1.50x |" in md
        assert "fd (not installed)" in md


def describe_records_on_disk():
    def it_round_trips_appended_records(tmp_path):
        rec = case_record("c", result({"find": [1.0] * 3}, [1.0] * 3), NO_STARTUP)
        append_record(tmp_path, rec)
        append_record(tmp_path, rec)
        assert read_records(tmp_path) == [rec, rec]


def describe_suite_mode():
    def it_times_every_native_for_the_requested_pairs(monkeypatch, tmp_path):
        monkeypatch.setenv("DIRSQL_BENCH_PAIRS", "4")
        monkeypatch.setenv("DIRSQL_BENCH_OUT", str(tmp_path))
        natives = Natives({"a": canned(3.0), "b": canned(1.0)}, [])
        out = paired(natives, canned(1.0), NO_STARTUP, pairs=2, screen=1)
        assert set(out.native_seconds) == {"a", "b"}
        assert len(out.native_seconds["a"]) == 4
        assert len(out.dirsql_seconds) == 4
        assert out.fastest == "b"

    def it_does_not_give_up_on_hopeless_pairs(monkeypatch, tmp_path):
        monkeypatch.setenv("DIRSQL_BENCH_PAIRS", "6")
        monkeypatch.setenv("DIRSQL_BENCH_OUT", str(tmp_path))
        out = paired(Natives({"a": canned(1.0)}, []), canned(50.0), NO_STARTUP)
        assert len(out.dirsql_seconds) == 6

    def it_records_the_case_instead_of_asserting_a_bar(monkeypatch, tmp_path):
        monkeypatch.setenv("DIRSQL_BENCH_PAIRS", "3")
        monkeypatch.setenv("DIRSQL_BENCH_OUT", str(tmp_path))
        over = result({"find": [1.0] * 3}, [9.0] * 3)
        assert_speed_of_light("case", over, NO_STARTUP)
        (rec,) = read_records(tmp_path)
        assert rec["name"] == "case"
        assert rec["ratio_vs_fastest"] == 9.0

    def it_is_off_without_the_environment(monkeypatch):
        monkeypatch.delenv("DIRSQL_BENCH_PAIRS", raising=False)
        monkeypatch.delenv("DIRSQL_BENCH_OUT", raising=False)
        out = paired(Natives({"a": canned(1.0)}, []), canned(1.0), NO_STARTUP, pairs=2, screen=1)
        assert len(out.dirsql_seconds) == 2
