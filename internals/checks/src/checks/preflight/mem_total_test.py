"""Colocated unit tests for the /proc/meminfo MemTotal parser."""

from checks.preflight.mem_total import mem_total_kb

MEMINFO = "MemFree:         1 kB\nMemTotal:       48369800 kB\nMemAvailable:    2 kB\n"


def describe_mem_total_kb():
    def it_reads_the_kib_figure_off_the_mem_total_line():
        assert mem_total_kb(MEMINFO) == 48369800

    def it_returns_none_when_no_line_names_mem_total():
        assert mem_total_kb("MemFree:         1 kB\n") is None

    def it_matches_the_field_name_at_the_start_of_the_line_only():
        assert mem_total_kb("SwapMemTotal:    5 kB\nMemTotal:        7 kB\n") == 7
