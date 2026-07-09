"""Host-platform detection for the node distcheck flow (#520).

A Python port of the host-row lookup in `packages/ts/src/platforms.ts`: enough
of each published target to reconstruct the host's `@dirsql/cli-<slug>`
sub-package (name, os/cpu constraints, binary suffix) and locate its staged
binary. Pure -- callers pass the detected `sys.platform` / `platform.machine()`
so both the mapping and the lookup are unit-testable.
"""
from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Platform:
    node_platform: str
    node_arch: str
    slug: str
    os: list[str]
    cpu: list[str]
    exe: bool

    @property
    def name(self) -> str:
        """CLI sub-package name (`@dirsql/cli-<slug>`)."""
        return f"@dirsql/cli-{self.slug}"

    @property
    def bin_name(self) -> str:
        return "dirsql.exe" if self.exe else "dirsql"


# Mirror of PLATFORMS in packages/ts/src/platforms.ts (the fields the node distcheck
# flow needs). Keep in lockstep with that file -- it is the release source of
# truth for the published sub-packages.
PLATFORMS: tuple[Platform, ...] = (
    Platform("linux", "x64", "linux-x64-gnu", ["linux"], ["x64"], False),
    Platform("linux", "arm64", "linux-arm64-gnu", ["linux"], ["arm64"], False),
    Platform("darwin", "x64", "darwin-x64", ["darwin"], ["x64"], False),
    Platform("darwin", "arm64", "darwin-arm64", ["darwin"], ["arm64"], False),
    Platform("win32", "x64", "win32-x64-msvc", ["win32"], ["x64"], True),
)

_ARCH = {
    "x86_64": "x64",
    "amd64": "x64",
    "arm64": "arm64",
    "aarch64": "arm64",
}

_BY_KEY = {(p.node_platform, p.node_arch): p for p in PLATFORMS}


def to_node_arch(machine: str) -> str:
    key = machine.lower()
    if key not in _ARCH:
        raise ValueError(f"unsupported machine {machine!r}; extend platforms.py")
    return _ARCH[key]


def find_host_platform(node_platform: str, node_arch: str) -> Platform:
    platform = _BY_KEY.get((node_platform, node_arch))
    if platform is None:
        raise ValueError(
            f"unsupported host {node_platform}-{node_arch}; "
            "add a row to PLATFORMS in packages/ts/src/platforms.ts (and here)"
        )
    return platform


def detect_host(sys_platform: str, machine: str) -> Platform:
    return find_host_platform(sys_platform, to_node_arch(machine))
