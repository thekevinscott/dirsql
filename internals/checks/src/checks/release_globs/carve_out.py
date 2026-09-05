"""The canonical publish globs for one package subtree (#944)."""

from __future__ import annotations

from .decide import NON_SHIPPING_DIRS, NON_SHIPPING_FILES


def carve_out(root: str) -> list[str]:
    """The canonical publish globs for the package subtree at ``root``.

    Two patterns, because a single extglob cannot cover both depths: the first
    matches files sitting directly in the package root, the second everything
    under its subdirectories.
    """
    return [
        f"{root}/!({'|'.join(NON_SHIPPING_FILES)})",
        f"{root}/!({'|'.join(NON_SHIPPING_DIRS)})/**",
    ]
