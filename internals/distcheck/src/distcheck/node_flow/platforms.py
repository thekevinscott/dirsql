"""Host-platform detection for the node distcheck flow (#520).

Reads the published-target table from `packages/ts/src/platforms.json`, the one
declarative copy shared with the TypeScript SDK, and keeps the fields this flow
needs to reconstruct the host's `@dirsql/lib-<slug>` sub-package (name, os/cpu
constraints, addon filename) and locate its staged addon. The lookup is pure --
callers pass the detected `sys.platform` / `platform.machine()`.
"""
from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

from .arch import to_node_arch

_LIB_PREFIX = "@dirsql/lib-"
_TABLE = Path(__file__).parents[5] / "packages" / "ts" / "src" / "platforms.json"


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


PLATFORMS: tuple[Platform, ...] = tuple(
    Platform(
        row["nodePlatform"],
        row["nodeArch"],
        row["libName"].removeprefix(_LIB_PREFIX),
        row["os"],
        row["cpu"],
    )
    for row in json.loads(_TABLE.read_text(encoding="utf-8"))
)

_BY_KEY = {(p.node_platform, p.node_arch): p for p in PLATFORMS}


def find_host_platform(node_platform: str, node_arch: str) -> Platform:
    platform = _BY_KEY.get((node_platform, node_arch))
    if platform is None:
        raise ValueError(
            f"unsupported host {node_platform}-{node_arch}; "
            "add a row to packages/ts/src/platforms.json"
        )
    return platform


def detect_host(sys_platform: str, machine: str) -> Platform:
    return find_host_platform(sys_platform, to_node_arch(machine))
