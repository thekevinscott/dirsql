"""Harness for the use-case speed-of-light tests.

Each use-case test asks one question of a generated corpus twice: once through
the fastest native pipeline (find, sort, join, awk, a single parser process)
and once through the real `dirsql` launcher. The rows must be identical and
dirsql's wall time, after subtracting the launcher's measured startup, must be
within ten percent of native. The bar is a ratio, never a millisecond number,
so it holds on any machine.

Native and dirsql run as alternating pairs (native first, then dirsql first,
and so on) after one discarded warm-up pair, so drift in machine speed hits
both sides alike. The verdict is the median of the per-pair ratios. Startup is
the median of `dirsql query 'SELECT 1'` runs with the first dropped, and
native's own is the median of `bash -c true` the same way; each is subtracted
from its side. A corpus too small to time is grown by doubling until the
native pipeline takes a second, unless the corpus ceiling stops it sooner.
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
from dataclasses import dataclass
from typing import NamedTuple

ONE_SECOND = 1.0
TOLERANCE = 1.1
STARTUP_PROBES = 11
PAIRS = 100
HOPELESS_STREAK = 3
NATIVE_ENV = {**os.environ, "LC_ALL": "C"}


class Startup(NamedTuple):
    dirsql: float
    native: float


@dataclass
class Paired:
    native_rows: object
    dirsql_rows: object
    native_seconds: list
    dirsql_seconds: list


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


def paired(native, dirsql, startup, pairs=PAIRS):
    """Alternate `native()` and `dirsql(timeout)`, native first on even pairs.

    Both return `(rows, seconds)`. The first pair is a discarded warm-up that
    also sets dirsql's timeout. A run that raises `TimeoutExpired` fails the
    test at once. Three hopeless pairs in a row end the series early: noise
    does not account for an order of magnitude, so more pairs cannot bring
    the median under the bar.
    """
    native_rows, native_warm = run_or_fail(native, "native")
    timeout = dirsql_timeout(native_warm)

    def run_dirsql():
        return run_or_fail(lambda: dirsql(timeout), "dirsql")

    dirsql_rows_, _ = run_dirsql()
    native_times, dirsql_times, streak = [], [], 0
    for index in range(pairs):
        if index % 2 == 0:
            native_rows, native_seconds = run_or_fail(native, "native")
            dirsql_rows_, dirsql_seconds = run_dirsql()
        else:
            dirsql_rows_, dirsql_seconds = run_dirsql()
            native_rows, native_seconds = run_or_fail(native, "native")
        native_times.append(native_seconds)
        dirsql_times.append(dirsql_seconds)
        streak = (
            streak + 1 if is_hopeless(native_seconds, dirsql_seconds, startup) else 0
        )
        if streak == HOPELESS_STREAK:
            break
    return Paired(native_rows, dirsql_rows_, native_times, dirsql_times)


def summarize(ratios):
    low, high = min(ratios), max(ratios)
    median = statistics.median(ratios)
    return low, median, high, (high - low) / median


def assert_speed_of_light(name, result, startup):
    ratios = pair_ratios(result.native_seconds, result.dirsql_seconds, startup)
    low, median, high, spread = summarize(ratios)
    line = (
        f"{name}: {len(ratios)} pairs, ratio min {low:.3f} median {median:.3f}"
        f" max {high:.3f} spread {spread:.0%}, native median"
        f" {statistics.median(result.native_seconds):.3f}s, startup"
        f" dirsql {startup.dirsql:.3f}s native {startup.native:.3f}s"
    )
    print(f"SPEED {line}")
    assert median <= TOLERANCE, f"{line}; median is over {TOLERANCE}x"
