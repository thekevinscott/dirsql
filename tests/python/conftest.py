import json
import os
import subprocess
import tempfile
import time
from pathlib import Path

import pytest

# tests/measure.py sets MEASURE to a results file and runs pytest once per repeat.
RESULTS = os.environ.get("MEASURE")
GNU_TIME = ["/usr/bin/time", "-f", "%M", "-o"]

began = {}


def pytest_configure(config):
    config.addinivalue_line("markers", "limits(ms, mb, baseline, cwd, cli): measured under tests/measure.py")


def status_mb(field):
    line = next(line for line in Path("/proc/self/status").read_text().splitlines() if line.startswith(field))
    return int(line.split()[1]) / 1024


def peak_mb(file):
    return int(Path(file).read_text().split()[-1]) / 1024


@pytest.hookimpl(wrapper=True)
def pytest_runtest_call(item):
    if not RESULTS:
        return (yield)
    marker = item.get_closest_marker("limits")
    if marker is None:
        pytest.fail("a measured test needs @pytest.mark.limits", pytrace=False)
    want = marker.kwargs
    cli = want.get("cli")
    with tempfile.NamedTemporaryFile() as peak:
        if cli is not None:
            # A CLI test's memory is its child's peak, so the launcher runs under GNU time.
            launcher = list(cli)
            cli[:0] = [*GNU_TIME, peak.name]
        if item.path not in began:
            # Resets this process's peak, so the peak read after a case belongs to the test.
            # Once per test, not per case: a later case reuses memory an earlier one freed.
            Path("/proc/self/clear_refs").write_text("5")
            began[item.path] = status_mb("VmRSS")
        start = time.perf_counter()
        try:
            result = yield
        finally:
            ms = (time.perf_counter() - start) * 1000
            grew = status_mb("VmHWM") - began[item.path]
            if cli is not None:
                cli[:] = launcher
        mb = peak_mb(peak.name) if cli is not None else grew

        start = time.perf_counter()
        subprocess.run(
            [*GNU_TIME, peak.name, "sh", "-c", want["baseline"]],
            cwd=want.get("cwd"), stdout=subprocess.DEVNULL, check=True,
        )
        base_ms = (time.perf_counter() - start) * 1000
        base_mb = peak_mb(peak.name)

    with open(RESULTS, "a") as results:
        line = {"file": str(item.path), "ms": ms, "mb": mb, "base_ms": base_ms, "base_mb": base_mb}
        results.write(json.dumps({**line, "limits": {"ms": want["ms"], "mb": want["mb"]}}) + "\n")
    return result
