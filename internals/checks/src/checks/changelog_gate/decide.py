"""Pure decision logic for the changelog-gate check (#494/#496).

Ported from the inline bash in `.github/workflows/changelog-check.yml`'s `case` dispatch,
trailer parsing, and diff line-counting -- unchanged in scope and precedence, just testable.
"""
from __future__ import annotations

import fnmatch

# Order does not matter across groups (rust/src, python/src, python/dirsql, ts/napi/src,
# ts/src are disjoint namespaces) -- only within a namespace, where the exclude for that
# namespace's test files must be checked before its general include. `is_sdk_code_change`
# checks all excludes before any include, which is equivalent to the original bash `case`
# (first-match-wins) precisely because no path can match two different namespaces' patterns.
_EXCLUDE_PATTERNS = (
    "packages/python/dirsql/*_test.py",
    "packages/ts/src/*.test.ts",
    "packages/ts/src/*.spec.ts",
)

_INCLUDE_PATTERNS = (
    "packages/rust/src/*",
    "packages/python/src/*",
    "packages/python/dirsql/*",
    "packages/ts/napi/src/*",
    "packages/ts/src/*",
    "Cargo.toml",
    "Cargo.lock",
    "packages/rust/Cargo.toml",
    "packages/ts/napi/Cargo.toml",
    "packages/python/Cargo.toml",
)


_FRAGMENT_PATTERN = "changelog.d/*.md"
_FRAGMENT_README = "changelog.d/README.md"


def is_changelog_fragment(path: str) -> bool:
    return fnmatch.fnmatch(path, _FRAGMENT_PATTERN) and path != _FRAGMENT_README


def changelog_fragments(paths) -> list[str]:
    return [path for path in paths if is_changelog_fragment(path)]


def is_sdk_code_change(path: str) -> bool:
    if any(fnmatch.fnmatch(path, pattern) for pattern in _EXCLUDE_PATTERNS):
        return False
    return any(fnmatch.fnmatch(path, pattern) for pattern in _INCLUDE_PATTERNS)


def any_sdk_code_changed(paths) -> bool:
    return any(is_sdk_code_change(path) for path in paths)


def extract_skip_trailers(trailer_output: str) -> list[str]:
    return [line for line in trailer_output.splitlines() if line.strip()]


def contains_skip_changelog_line(commit_messages: str) -> bool:
    """True if any commit-message line is a `skip-changelog:` directive.

    Detects an *attempted* skip-changelog independent of git's trailer parser,
    so the gate can tell a malformed (e.g. blank-line-split) trailer apart from
    no skip at all and emit a targeted fix message.
    """
    return any(
        line.strip().lower().startswith("skip-changelog:")
        for line in commit_messages.splitlines()
    )


def count_added_lines(diff_text: str) -> int:
    added = 0
    for line in diff_text.splitlines():
        if len(line) >= 2 and line[0] == "+" and line[1] != "+" and line[1:].strip():
            added += 1
    return added
