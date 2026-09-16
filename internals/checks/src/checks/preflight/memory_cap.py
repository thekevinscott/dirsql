"""Decide how the mutation gate is boxed in.

A mutant that allocates without bound is ordinary output of a mutation engine,
and the engines cap wall time, not memory. Running the gate inside its own
cgroup scope lets the kernel kill that scope alone; the engine records the
killed mutant as caught and moves on. Half of MemTotal clears the unmutated
baseline build plus the linker with room to spare; no swap, so a runaway
cannot page out everyone else's working set on the way to the limit.
"""

from __future__ import annotations

from dataclasses import dataclass

from .host import Host


@dataclass
class MemoryCap:
    prefix: list[str]
    # Why `prefix` is empty, for the one line preflight prints in that case.
    skipped: str = ""


def memory_cap(host: Host) -> MemoryCap:
    if not host.systemd_run:
        return MemoryCap([], "systemd-run is not on PATH")
    if host.mem_total_kb is None:
        return MemoryCap([], "MemTotal is not readable from /proc/meminfo")
    limit = f"MemoryMax={host.mem_total_kb // 2048}M"
    return MemoryCap(["systemd-run", "--user", "--scope", "-p", limit, "-p", "MemorySwapMax=0"])
