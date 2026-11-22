import statistics
import subprocess
import tempfile
import time

import psutil
import pytest

PAIRS = 5
STEP = "__pytest_repeat_step_number"
GNU_TIME = ["/usr/bin/time", "-f", "%M"]

pairs = {}


def pytest_addoption(parser):
    parser.addoption(
        "--measure", choices=["speed", "memory"],
        help="enforce @pytest.mark.limits; separate passes, because tracking memory slows the test",
    )


def pytest_configure(config):
    config.addinivalue_line(
        "markers", "limits(ms_max, mb_max, baseline, cwd, cli): enforced with --measure"
    )
    if config.getoption("--measure"):
        # pytest-repeat: each repeat is one test run followed by one baseline run.
        config.option.count = PAIRS


def pytest_collection_modifyitems(config, items):
    if config.getoption("--measure") != "memory":
        return
    for item in items:
        limits = item.get_closest_marker("limits")
        if limits and "mb_max" in limits.kwargs:
            item.add_marker(pytest.mark.limit_memory(f"{limits.kwargs['mb_max']['max']} MB"))


def baseline_ms(command, cwd):
    start = time.perf_counter()
    subprocess.run(["sh", "-c", command], cwd=cwd, stdout=subprocess.DEVNULL, check=True)
    return (time.perf_counter() - start) * 1000


def baseline_mb(command, cwd):
    # GNU time, because a forked child's own peak starts at pytest's size.
    timed = subprocess.run(
        [*GNU_TIME, "sh", "-c", command],
        cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, check=True,
    )
    return int(timed.stderr.strip().splitlines()[-1]) / 1024


def over(what, got, limit, unit):
    return [f"{what} {got:.1f}{unit}, limit {limit}{unit}"] if limit is not None and got > limit else []


def record(item, got, base):
    params = {name: value for name, value in item.callspec.params.items() if name != STEP}
    seen = pairs.setdefault((item.path, item.originalname, repr(params)), [])
    seen.append((got, base))
    return seen


def judge(item, seen, limits, unit, verb):
    got, base = seen[-1]
    failures = over(verb, got, limits["max"], unit)
    lines = [f"test {got:.1f}{unit}; baseline {base:.1f}{unit}"]
    if len(seen) == PAIRS:
        got, base = (statistics.mean(column) for column in zip(*seen))
        lines.append(f"mean of {PAIRS} pairs: test {got:.1f}{unit}; baseline {base:.1f}{unit}; {got / base:.1f}x")
        failures += over(f"mean {verb}", got, limits.get("mean"), unit)
        failures += over("mean was", got / base, limits.get("times_baseline"), "x baseline")
    item.add_report_section("call", "measure", "\n".join(lines))
    if failures:
        pytest.fail("; ".join(failures), pytrace=False)


@pytest.hookimpl(wrapper=True)
def pytest_runtest_call(item):
    limits = item.get_closest_marker("limits")
    mode = item.config.getoption("--measure")
    if not (limits and mode):
        return (yield)
    want = limits.kwargs
    if mode == "speed":
        start = time.perf_counter()
        result = yield
        ms = (time.perf_counter() - start) * 1000
        judge(item, record(item, ms, baseline_ms(want["baseline"], want.get("cwd"))), want["ms_max"], " ms", "took")
        return result

    cli = want.get("cli")
    process = psutil.Process()
    with tempfile.NamedTemporaryFile("r") as peak:
        if cli is not None:
            # A CLI test's memory is its child's peak, so the launcher runs under GNU time.
            launcher = list(cli)
            cli[:0] = [*GNU_TIME, "-o", peak.name]
        rss = process.memory_info().rss
        try:
            result = yield
        finally:
            grew = (process.memory_info().rss - rss) / 2**20
            if cli is not None:
                cli[:] = launcher
        mb = int(peak.read().split()[-1]) / 1024 if cli is not None else grew
    judge(item, record(item, mb, baseline_mb(want["baseline"], want.get("cwd"))), want["mb_max"], " MB", "used")
    return result
