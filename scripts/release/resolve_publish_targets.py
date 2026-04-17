"""Resolve which packages to publish for a given release dispatch.

Consumed by .github/workflows/release.yml. Writes the four *_changed flags
to GITHUB_OUTPUT when run as a script. Keep all branching logic here, not
in the workflow YAML -- the YAML should be a thin wrapper.
"""

from __future__ import annotations

import fnmatch
import os
import sys
from dataclasses import asdict, dataclass


@dataclass(frozen=True)
class Flags:
    rust: bool
    python: bool
    js: bool
    docs: bool


# File-pattern buckets, mirroring the previous shell logic. A single file
# can flip multiple buckets (e.g. a .md under packages/ts/ would flip both
# js and docs) -- that's intentional.
_RUST_GLOBS = (
    "packages/rust/*",
    "packages/rust/**/*",
    "packages/python/src/*",
    "packages/python/src/**/*",
    "Cargo.lock",
)
_PYTHON_GLOBS = (
    "packages/python/**/*.py",
    "packages/python/pyproject.toml",
)
_JS_GLOBS = (
    "packages/ts/*",
    "packages/ts/**/*",
)
_DOCS_GLOBS = (
    "README.md",
    "*.md",
    "**/*.md",
)


def _matches_any(path: str, patterns: tuple[str, ...]) -> bool:
    return any(fnmatch.fnmatchcase(path, p) for p in patterns)


def resolve(
    *,
    event_name: str,
    publish_mode: str,
    custom_python: bool,
    custom_rust: bool,
    custom_js: bool,
    latest_tag: str,
    changed_files: list[str],
) -> Flags:
    # Only workflow_dispatch honors publish_mode. Scheduled/push triggers
    # always auto-detect -- they can't carry inputs.
    mode = publish_mode if event_name == "workflow_dispatch" else "changed"

    if mode == "all":
        return Flags(rust=True, python=True, js=True, docs=True)

    if mode == "custom":
        # Verbatim: no cascade, no docs. Operator is responsible for their
        # own selection.
        return Flags(
            rust=custom_rust,
            python=custom_python,
            js=custom_js,
            docs=False,
        )

    # mode == "changed"
    if not latest_tag:
        # First-ever release -- treat all packages as changed.
        return Flags(rust=True, python=True, js=True, docs=True)

    rust = any(_matches_any(f, _RUST_GLOBS) for f in changed_files)
    python = any(_matches_any(f, _PYTHON_GLOBS) for f in changed_files)
    js = any(_matches_any(f, _JS_GLOBS) for f in changed_files)
    docs = any(_matches_any(f, _DOCS_GLOBS) for f in changed_files)
    return Flags(rust=rust, python=python, js=js, docs=docs)


def _env_bool(name: str) -> bool:
    return os.environ.get(name, "").strip().lower() == "true"


def _write_github_output(flags: Flags) -> None:
    # Map dataclass field names to the GITHUB_OUTPUT keys the workflow
    # consumes.
    out_path = os.environ.get("GITHUB_OUTPUT")
    lines = [f"{k}_changed={'true' if v else 'false'}" for k, v in asdict(flags).items()]
    text = "\n".join(lines) + "\n"
    if out_path:
        with open(out_path, "a", encoding="utf-8") as fh:
            fh.write(text)
    else:
        sys.stdout.write(text)


def main() -> int:
    changed = [
        line.strip()
        for line in os.environ.get("CHANGED_FILES", "").splitlines()
        if line.strip()
    ]
    flags = resolve(
        event_name=os.environ.get("EVENT_NAME", ""),
        publish_mode=os.environ.get("PUBLISH_MODE", "all"),
        custom_python=_env_bool("CUSTOM_PYTHON"),
        custom_rust=_env_bool("CUSTOM_RUST"),
        custom_js=_env_bool("CUSTOM_JS"),
        latest_tag=os.environ.get("LATEST_TAG", ""),
        changed_files=changed,
    )
    _write_github_output(flags)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
