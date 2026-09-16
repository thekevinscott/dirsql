"""What the memory cap needs to know about the machine preflight runs on."""

from __future__ import annotations

import shutil
from dataclasses import dataclass

from .mem_total import mem_total_kb

MEMINFO = "/proc/meminfo"


@dataclass
class Host:
    systemd_run: bool
    # None where the kernel exposes no meminfo (macOS, and any non-Linux).
    mem_total_kb: int | None


def detect_host() -> Host:
    try:
        with open(MEMINFO, encoding="utf-8") as handle:
            total = mem_total_kb(handle.read())
    except OSError:
        total = None
    return Host(systemd_run=shutil.which("systemd-run") is not None, mem_total_kb=total)
