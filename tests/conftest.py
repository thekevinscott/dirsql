import time

import pytest


def pytest_addoption(parser):
    parser.addoption("--measure", action="store_true", help="enforce @pytest.mark.limits")


def pytest_configure(config):
    config.addinivalue_line("markers", "limits(ms_max, mb_max): enforced with --measure")


def pytest_collection_modifyitems(config, items):
    if not config.getoption("--measure"):
        return
    for item in items:
        limits = item.get_closest_marker("limits")
        if limits and "mb_max" in limits.kwargs:
            item.add_marker(pytest.mark.limit_memory(f"{limits.kwargs['mb_max']['max']} MB"))


@pytest.hookimpl(wrapper=True)
def pytest_runtest_call(item):
    limits = item.get_closest_marker("limits")
    if not (limits and item.config.getoption("--measure")):
        return (yield)
    start = time.perf_counter()
    result = yield
    ms = (time.perf_counter() - start) * 1000
    ms_max = limits.kwargs["ms_max"]["max"]
    item.user_properties.append(("ms", round(ms, 2)))
    if ms > ms_max:
        pytest.fail(f"took {ms:.1f} ms, limit {ms_max} ms", pytrace=False)
    return result
