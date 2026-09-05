"""The pull-request path filter a workflow declares (#944)."""

from __future__ import annotations


def pull_request_paths(workflow: dict) -> list[str]:
    """The `on.pull_request.paths` filter, or empty when the workflow has none.

    YAML 1.1 resolves a bare ``on`` key to the boolean ``True``, which is how
    PyYAML hands back every GitHub workflow; the string key is accepted too so a
    quoted ``"on":`` reads the same.
    """
    triggers = workflow.get(True, workflow.get("on")) or {}
    return (triggers.get("pull_request") or {}).get("paths") or []
