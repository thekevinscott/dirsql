"""Manifest reading for the declared-deps check (#782)."""

from __future__ import annotations

from .gate import requirement_name


def declared(manifest: dict) -> tuple[set[str], set[str]]:
    """(runtime, dev) distribution names the manifest declares."""
    runtime = {requirement_name(s) for s in manifest.get("project", {}).get("dependencies", [])}
    groups = manifest.get("dependency-groups", {})
    dev = {requirement_name(s) for s in groups.get("dev", [])}
    return runtime, dev
