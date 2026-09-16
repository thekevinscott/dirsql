"""Colocated unit tests for the mutation gate's memory cap."""

from checks.preflight.memory_cap import MemoryCap, memory_cap


class Host:
    """Stand-in for `host.Host` -- a value record, faked rather than imported."""

    def __init__(self, systemd_run, mem_total_kb):
        self.systemd_run = systemd_run
        self.mem_total_kb = mem_total_kb


def describe_memory_cap():
    def it_caps_a_user_scope_at_half_of_mem_total_with_no_swap():
        assert memory_cap(Host(True, 48369800)) == MemoryCap(
            [
                *["systemd-run", "--user", "--scope"],
                *["-p", "MemoryMax=23618M", "-p", "MemorySwapMax=0"],
            ]
        )

    def it_rounds_the_cap_down_to_whole_mebibytes():
        assert "MemoryMax=1M" in memory_cap(Host(True, 4095)).prefix

    def it_reports_nothing_skipped_when_it_caps():
        assert memory_cap(Host(True, 48369800)).skipped == ""

    def it_skips_when_systemd_run_is_not_on_path():
        cap = memory_cap(Host(False, 48369800))
        assert cap.prefix == []
        assert cap.skipped == "systemd-run is not on PATH"

    def it_skips_when_mem_total_is_unknown():
        cap = memory_cap(Host(True, None))
        assert cap.prefix == []
        assert cap.skipped == "MemTotal is not readable from /proc/meminfo"
