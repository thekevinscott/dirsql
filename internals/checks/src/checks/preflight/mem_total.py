"""Parse the kernel's total-memory figure out of `/proc/meminfo` text."""

from __future__ import annotations


def mem_total_kb(meminfo: str) -> int:
    for line in meminfo.splitlines():
        if line.startswith("MemTotal:"):
            return int(line.split()[1])
    raise ValueError("no MemTotal line in meminfo")
