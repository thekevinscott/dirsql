import subprocess
import time

import pytest


def pytest_addoption(parser):
    parser.addoption("--measure", action="store_true", help="enforce @pytest.mark.limits")


def pytest_configure(config):
    config.addinivalue_line(
        "markers", "limits(ms_max, mb_max, baseline, cwd): enforced with --measure"
    )


def pytest_collection_modifyitems(config, items):
    if not config.getoption("--measure"):
        return
    for item in items:
        limits = item.get_closest_marker("limits")
        if limits and "mb_max" in limits.kwargs:
            item.add_marker(pytest.mark.limit_memory(f"{limits.kwargs['mb_max']['max']} MB"))


def run_baseline(command, cwd):
    # GNU time, because a forked child's own peak starts at pytest's size.
    start = time.perf_counter()
    timed = subprocess.run(
        ["/usr/bin/time", "-f", "%M", "sh", "-c", command],
        cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, check=True,
    )
    ms = (time.perf_counter() - start) * 1000
    return ms, int(timed.stderr.strip().splitlines()[-1]) / 1024


@pytest.hookimpl(wrapper=True)
def pytest_runtest_call(item):
    limits = item.get_closest_marker("limits")
    if not (limits and item.config.getoption("--measure")):
        return (yield)
    ms_limits = limits.kwargs["ms_max"]
    start = time.perf_counter()
    result = yield
    ms = (time.perf_counter() - start) * 1000
    lines = [f"test: {ms:.1f} ms"]
    failures = []
    if ms > ms_limits["max"]:
        failures.append(f"took {ms:.1f} ms, limit {ms_limits['max']} ms")
    if "baseline" in limits.kwargs:
        base_ms, base_mb = run_baseline(limits.kwargs["baseline"], limits.kwargs.get("cwd"))
        lines.append(f"baseline: {base_ms:.1f} ms, {base_mb:.1f} MB peak")
        lines.append(f"ratio: {ms / base_ms:.1f}x baseline time")
        times = ms_limits.get("times_baseline")
        if times is not None and ms > base_ms * times:
            failures.append(f"took {ms / base_ms:.1f}x the baseline, limit {times}x")
    item.add_report_section("call", "measure", "\n".join(lines))
    if failures:
        pytest.fail("; ".join(failures), pytrace=False)
    return result
