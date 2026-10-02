"""Harness for the use-case speed-of-light tests.

Each use-case test asks one question of a generated corpus twice: once through
the fastest native pipeline (find, sort, join, awk, a single parser process)
and once through the real `dirsql` launcher. The rows must be identical and
dirsql's wall time, after subtracting the launcher's measured startup, must be
within ten percent of native. The bar is a ratio, never a millisecond number,
so it holds on any machine.

Startup is the mean of ten `dirsql --help` runs; `--help` and `SELECT 1` cost
the same, so it stands in for the fixed per-invocation cost. A corpus too
small to time is grown by doubling until the native pipeline takes at least a
second, so the comparison is above the noise floor. Each side is run three
times and the fastest run counts.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import time

ONE_SECOND = 1.0
TOLERANCE = 1.1
STARTUP_PROBES = 10
BEST_OF = 3


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
        text=True,
        timeout=timeout,
        env=env,
        check=False,
    )
    return proc, time.perf_counter() - started


def startup_seconds(argv=None, cwd=None) -> float:
    argv = [cli(), "--help"] if argv is None else argv
    samples = []
    for _ in range(STARTUP_PROBES):
        proc, seconds = timed(argv, cwd or ".", timeout=60)
        assert proc.returncode == 0, proc.stderr
        samples.append(seconds)
    return sum(samples) / len(samples)


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
        built = size
        _, seconds = native()
        if seconds >= ONE_SECOND or size >= ceiling:
            return size
        size *= 2


def best_of(run, name, hopeless=None):
    """Run `run` three times; return the rows and wall time of the fastest.

    `run` returns `(rows, seconds)`. A run that raises `TimeoutExpired` fails
    the test immediately with the timeout it blew through. A run slower than
    `hopeless` seconds ends the series early: noise does not account for an
    order of magnitude, so no retry could bring the best under the bar.
    """
    fastest = None
    for _ in range(BEST_OF):
        try:
            rows, seconds = run()
        except subprocess.TimeoutExpired as expired:
            raise AssertionError(
                f"{name} did not finish within {expired.timeout:.0f}s"
            ) from None
        if fastest is None or seconds < fastest[1]:
            fastest = (rows, seconds)
        if hopeless is not None and seconds > hopeless:
            break
    return fastest


def hopeless_seconds(native_seconds: float, startup: float) -> float:
    return startup + 10 * TOLERANCE * native_seconds


def dirsql_timeout(native_seconds: float) -> float:
    return max(60.0, 30 * native_seconds)


def assert_speed_of_light(name, native_seconds, dirsql_seconds, startup):
    measured = dirsql_seconds - startup
    budget = TOLERANCE * native_seconds
    assert measured <= budget, (
        f"{name}: dirsql {dirsql_seconds:.3f}s minus startup {startup:.3f}s"
        f" = {measured:.3f}s, over {TOLERANCE}x native {native_seconds:.3f}s"
        f" = {budget:.3f}s"
    )
