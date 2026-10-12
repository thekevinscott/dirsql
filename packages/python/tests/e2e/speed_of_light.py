"""Harness for the use-case speed-of-light tests.

Each test asks one question of a generated corpus through every plausible
native pipeline (find, fd, rg, xargs -P, a single parser process) and once
through the real `dirsql` launcher. Every native's rows must equal dirsql's.
dirsql's wall time, after subtracting the launcher's measured startup, must be
within ten percent of the case's bar native, `find` or the case's own first
native unless the case passes `bar=FASTEST`. The fastest native's ratio is
printed either way. The bar is a ratio, never a millisecond number, so it holds
on any machine. A native whose tool is not installed is printed as SKIPPED,
never dropped quietly.

A few screening pairs time every native to find the fastest; the measured
pairs then run only the fastest and the bar native. Pairs alternate (natives
first, then dirsql first, and so on) after one discarded warm-up pair, so
drift in machine speed hits every side alike. The verdict is the median of the
per-pair ratios. Startup is the median of `dirsql query 'SELECT 1'` runs with
the first dropped, and native's own is the median of `bash -c true` the same
way; each is subtracted from its side. A corpus too small to time is grown by
doubling until the native pipeline takes a second, unless the corpus ceiling
stops it sooner.
Output is captured as bytes and decoded after the clock stops.
"""

from __future__ import annotations

import json
import os
import shutil
import statistics
import subprocess
import tempfile
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from typing import NamedTuple

from .bench_report import append_record, case_record

ONE_SECOND = 1.0
TOLERANCE = 1.1
STARTUP_PROBES = 11
PAIRS = 15
SCREEN_PAIRS = 5
FASTEST = "fastest"
HOPELESS_STREAK = 3
NATIVE_ENV = {**os.environ, "LC_ALL": "C"}


class Startup(NamedTuple):
    dirsql: float
    native: float


class Native(NamedTuple):
    """One native pipeline. `needs` lists the names its tool goes by, first
    installed wins; `script` has `{bin}` where that name goes."""

    name: str
    needs: tuple
    script: str


class Natives(NamedTuple):
    """`bar` names the native dirsql is held to: the first one by default, or
    `FASTEST`."""

    runs: dict
    skipped: list
    bar: str = ""


class Series(NamedTuple):
    rows: dict
    dirsql_rows: object
    native_seconds: dict
    dirsql_seconds: list


@dataclass
class Paired:
    native_rows: dict
    dirsql_rows: object
    native_seconds: dict
    dirsql_seconds: list
    skipped: list
    screen_seconds: dict = field(default_factory=dict)
    screen_dirsql: list = field(default_factory=list)
    fastest: str = ""
    bar: str = ""


def suite_pairs():
    """The pair count when `just bench-suite` drives the run, else None."""
    count = os.environ.get("DIRSQL_BENCH_PAIRS")
    return int(count) if count and os.environ.get("DIRSQL_BENCH_OUT") else None


def suite_only(*specs):
    """Natives the performance suite times that the e2e ratchet does not."""
    return list(specs) if suite_pairs() else []


def cli() -> str:
    dirsql = shutil.which("dirsql")
    assert dirsql is not None, (
        "`dirsql` console script not on PATH -- run `uv run maturin develop`"
    )
    return dirsql


def timed(argv, cwd, timeout, env=None):
    started = time.perf_counter()
    proc = subprocess.run(
        argv,
        cwd=str(cwd),
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=timeout,
        env=env,
        check=False,
    )
    seconds = time.perf_counter() - started
    proc.stdout = proc.stdout.decode()
    proc.stderr = proc.stderr.decode()
    return proc, seconds


def timed_native(argv, cwd, timeout=600):
    return timed(argv, cwd, timeout, env=NATIVE_ENV)


def sorted_lines(proc):
    return sorted(proc.stdout.splitlines())


def resolve_natives(specs, which=shutil.which):
    runnable, skipped = [], []
    for spec in specs:
        binary = next((n for n in spec.needs if which(n)), None)
        if binary is None:
            skipped.append(spec.name)
        else:
            runnable.append((spec.name, spec.script.replace("{bin}", binary)))
    assert runnable, f"no native installed: {[s.name for s in specs]}"
    return runnable, skipped


def shell_natives(root, specs, parse=sorted_lines, shell="bash", bar="") -> Natives:
    def runner(script):
        def run():
            proc, seconds = timed_native([shell, "-c", script], root)
            assert proc.returncode == 0, proc.stderr
            return parse(proc), seconds

        return run

    runnable, skipped = resolve_natives(specs)
    return Natives({name: runner(script) for name, script in runnable}, skipped, bar)


AGG_AWK = (
    '{ n[$1]++; s[$1] += $2 } END { for (k in n) print k "\\t" n[k] "\\t" s[k] }\n'
)
MERGE_AWK = (
    '{ n[$1] += $2; s[$1] += $3 } END { for (k in n) print k "\\t" n[k] "\\t" s[k] }\n'
)


def fd(args):
    return Native("fd", ("fd", "fdfind"), "{bin} --no-ignore --type f " + args)


def rg(args):
    return Native("rg", ("rg",), "{bin} --files --no-ignore " + args)


def globstar(pattern):
    trim = "cut -c3-" if pattern.startswith("./") else "cat"
    return Native(
        "bash-globstar",
        ("bash",),
        '{bin} -O globstar -O nullglob -c \'printf "%s\\n" ' + pattern + f"' | {trim}",
    )


def baseline(natives: Natives) -> Callable:
    return next(iter(natives.runs.values()))


def median_after_first(samples):
    return statistics.median(samples[1:])


def probe_seconds(argv, cwd, env=None) -> float:
    samples = []
    for _ in range(STARTUP_PROBES):
        proc, seconds = timed(argv, cwd, timeout=60, env=env)
        assert proc.returncode == 0, proc.stderr
        samples.append(seconds)
    return median_after_first(samples)


def startup_seconds(argv=None, cwd=None) -> Startup:
    argv = [cli(), "query", "SELECT 1"] if argv is None else argv
    with tempfile.TemporaryDirectory() as empty:
        dirsql = probe_seconds(argv, cwd or empty)
        native = probe_seconds(["bash", "-c", "true"], empty, NATIVE_ENV)
    return Startup(dirsql, native)


def dirsql_rows(proc, columns):
    assert proc.returncode == 0, (
        f"dirsql failed: stdout={proc.stdout!r} stderr={proc.stderr!r}"
    )
    return [tuple(row[column] for column in columns) for row in json.loads(proc.stdout)]


def grow_until_native_takes_a_second(build, native, start, ceiling):
    """Grow the corpus by doubling until `native` runs for a second.

    `build(lo, hi)` adds items `lo` through `hi - 1` to the corpus already
    holding `lo` items, so each doubling writes only the new half.
    """
    built, size = 0, start
    while True:
        build(built, size)
        os.sync()
        built = size
        _, seconds = native()
        if seconds >= ONE_SECOND or size >= ceiling:
            return size
        size *= 2


def dirsql_timeout(native_seconds: float) -> float:
    return max(60.0, 30 * native_seconds)


def pair_ratios(native_seconds, dirsql_seconds, startup):
    ratios = []
    for native, dirsql in zip(native_seconds, dirsql_seconds):
        net = native - startup.native
        assert net > 0, (
            f"native {native:.3f}s is under its startup {startup.native:.3f}s"
        )
        ratios.append((dirsql - startup.dirsql) / net)
    return ratios


def is_hopeless(native, dirsql, startup):
    return dirsql - startup.dirsql > 10 * TOLERANCE * (native - startup.native)


def run_or_fail(run, name):
    try:
        return run()
    except subprocess.TimeoutExpired as expired:
        raise AssertionError(
            f"{name} did not finish within {expired.timeout:.0f}s"
        ) from None


def paired(natives, dirsql, startup, pairs=PAIRS, screen=SCREEN_PAIRS):
    """Screen every native, then time dirsql against the bar and the fastest.

    The first pair is a discarded warm-up that also sets dirsql's timeout.
    `screen` alternating pairs run every native to find the fastest by median
    time; `pairs` alternating pairs then run only the fastest and the bar
    native (the same one unless the bar is pinned elsewhere). A run that
    raises `TimeoutExpired` fails the test at once. Three hopeless pairs in a
    row, judged against the pair's fastest native, end the measured series
    early: noise does not account for an order of magnitude, so more pairs
    cannot bring the median under the bar.
    """
    runs = natives.runs
    rows = {}
    warm = []
    for name, run in runs.items():
        rows[name], seconds = run_or_fail(run, name)
        warm.append(seconds)
    timeout = dirsql_timeout(max(warm))

    def run_dirsql():
        return run_or_fail(lambda: dirsql(timeout), "dirsql")

    run_dirsql()
    everyone = suite_pairs()
    if everyone:
        result = run_pairs(runs, run_dirsql, everyone)
        fastest = fastest_by_median(result.native_seconds)
        return Paired(
            {**rows, **result.rows},
            result.dirsql_rows,
            result.native_seconds,
            result.dirsql_seconds,
            natives.skipped,
            fastest=fastest,
            bar=fastest,
        )
    screened = run_pairs(runs, run_dirsql, screen)
    rows.update(screened.rows)
    fastest = fastest_by_median(screened.native_seconds)
    bar = fastest if natives.bar == FASTEST else natives.bar or next(iter(runs))
    measured = {name: runs[name] for name in dict.fromkeys([fastest, bar])}
    result = run_pairs(measured, run_dirsql, pairs, startup)
    rows.update(result.rows)
    return Paired(
        rows,
        result.dirsql_rows,
        result.native_seconds,
        result.dirsql_seconds,
        natives.skipped,
        screened.native_seconds,
        screened.dirsql_seconds,
        fastest,
        bar,
    )


def run_pairs(runs, run_dirsql, count, startup=None):
    """Alternate every run in `runs` with dirsql, runs first on even pairs."""
    rows = {}
    dirsql_rows_ = None
    native_times = {name: [] for name in runs}
    dirsql_times, streak = [], 0
    for index in range(count):
        order = list(runs)
        if index % 2:
            order.reverse()
        this_pair = {}
        if index % 2 == 0:
            for name in order:
                rows[name], this_pair[name] = run_or_fail(runs[name], name)
            dirsql_rows_, dirsql_seconds = run_dirsql()
        else:
            dirsql_rows_, dirsql_seconds = run_dirsql()
            for name in order:
                rows[name], this_pair[name] = run_or_fail(runs[name], name)
        for name, seconds in this_pair.items():
            native_times[name].append(seconds)
        dirsql_times.append(dirsql_seconds)
        if startup is None:
            continue
        hopeless = is_hopeless(min(this_pair.values()), dirsql_seconds, startup)
        streak = streak + 1 if hopeless else 0
        if streak == HOPELESS_STREAK:
            break
    return Series(rows, dirsql_rows_, native_times, dirsql_times)


def summarize(ratios):
    low, high = min(ratios), max(ratios)
    median = statistics.median(ratios)
    return low, median, high, (high - low) / median


def fastest_by_median(native_seconds):
    return min(native_seconds, key=lambda name: statistics.median(native_seconds[name]))


def agreed_rows(result):
    names = list(result.native_rows)
    first = names[0]
    for name in names[1:]:
        assert result.native_rows[name] == result.native_rows[first], (
            f"{name} disagrees with {first}"
        )
    return result.native_rows[first]


def ratio_line(label, native, native_seconds, dirsql_seconds, startup):
    ratios = pair_ratios(native_seconds, dirsql_seconds, startup)
    low, median, high, spread = summarize(ratios)
    line = (
        f"{label} vs {native}: {len(ratios)} pairs, ratio min {low:.3f}"
        f" median {median:.3f} max {high:.3f} spread {spread:.0%}, native"
        f" median {statistics.median(native_seconds):.3f}s"
    )
    return median, line


def assert_speed_of_light(name, result, startup):
    """Print every native's screening time and ratio, then hold dirsql to the bar.

    The bar is `result.bar`; the fastest native's ratio is printed beside it
    and not asserted unless the bar is that native. Under `just bench-suite`
    the case is recorded and nothing is asserted: the suite reports.
    """
    if suite_pairs():
        append_record(
            os.environ["DIRSQL_BENCH_OUT"], case_record(name, result, startup)
        )
        return
    lines = []
    for native, seconds in result.screen_seconds.items():
        _, line = ratio_line(
            f"{name} screen", native, seconds, result.screen_dirsql, startup
        )
        lines.append(line + (" [fastest]" if native == result.fastest else ""))
    lines.extend(f"{name}: SKIPPED {tool} (not installed)" for tool in result.skipped)
    verdicts = {}
    for role, native in (("bar", result.bar), ("fastest", result.fastest)):
        verdicts[role] = ratio_line(
            name, native, result.native_seconds[native], result.dirsql_seconds, startup
        )
        lines.append(f"{role} {verdicts[role][1]}")
    lines.append(
        f"{name}: startup dirsql {startup.dirsql:.3f}s native {startup.native:.3f}s,"
        f" load {os.getloadavg()[0]:.1f}"
    )
    for line in lines:
        print(f"SPEED {line}")
    median, line = verdicts["bar"]
    assert median <= TOLERANCE, (
        f"bar native {result.bar}: {line}; median is over {TOLERANCE}x"
    )
