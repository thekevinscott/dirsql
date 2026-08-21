"""Host-platform detection for the node distcheck flow (#520).

A Python port of the host-row lookup in `packages/ts/src/platforms.ts`: enough
of each published target to reconstruct the host's `@dirsql/lib-<slug>`
sub-package (name, os/cpu constraints, addon filename) and locate its staged
addon. Pure -- callers pass the detected `sys.platform` / `platform.machine()`
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

    @property
    def name(self) -> str:
        """napi addon sub-package name (`@dirsql/lib-<slug>`).

        Since #739 this is the only per-platform family: the addon carries
        the CLI (`runCli`), so no `@dirsql/cli-<slug>` is published.
        """
        return f"@dirsql/lib-{self.slug}"

    @property
    def addon_name(self) -> str:
        """The `.node` filename napi emits for this triple."""
        return f"dirsql.{self.slug}.node"


# The subset of PLATFORMS in packages/ts/src/platforms.ts this flow needs -- that
# file is the release source of truth for the published sub-packages, and its
# `triple` and `libc` reconstruct nothing here. `dirsql-checks platforms-mirror`
# holds the two tables to that subset, in both directions.
PLATFORMS: tuple[Platform, ...] = (
    Platform("linux", "x64", "linux-x64-gnu", ["linux"], ["x64"]),
    Platform("linux", "arm64", "linux-arm64-gnu", ["linux"], ["arm64"]),
    Platform("darwin", "x64", "darwin-x64", ["darwin"], ["x64"]),
    Platform("darwin", "arm64", "darwin-arm64", ["darwin"], ["arm64"]),
    Platform("win32", "x64", "win32-x64-msvc", ["win32"], ["x64"]),
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
