"""`platform.machine()` -> node's `process.arch` vocabulary."""
from __future__ import annotations

_ARCH = {
    "x86_64": "x64",
    "amd64": "x64",
    "arm64": "arm64",
    "aarch64": "arm64",
}


def to_node_arch(machine: str) -> str:
    key = machine.lower()
    if key not in _ARCH:
        raise ValueError(f"unsupported machine {machine!r}; extend platforms.py")
    return _ARCH[key]
