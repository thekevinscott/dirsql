"""Pure decision logic for the changelog-gate check (#494/#496).

Since #565 the gate is **per-package, fragments-only**: a PR that changes a
package's public-facing SDK source must add a fragment under that package's
own ``packages/<pkg>/changelog.d/`` (named ``YYYY-MM-DD-<slug>.md``). The old
root ``changelog.d/`` directory and the direct ``CHANGELOG.md`` edit are no
longer accepted -- ``CHANGELOG.md`` is a frozen pointer stub (#563).
"""

import fnmatch
import re

# The three SDK packages that own a changelog. Directory slug under packages/.
PACKAGES = ("python", "ts", "rust")

# Test files never require a changelog entry; checked before the includes.
_TEST_EXCLUDES = (
    "packages/python/dirsql/*_test.py",
    "packages/ts/src/*.test.ts",
    "packages/ts/src/*.spec.ts",
)

# Public-facing SDK source, grouped by the package that owns it. The top-level
# Cargo manifest/lock belong to the Rust core.
_PACKAGE_SOURCES = {
    "rust": (
        "packages/rust/src/*",
        "packages/rust/Cargo.toml",
        "Cargo.toml",
        "Cargo.lock",
    ),
    "python": (
        "packages/python/src/*",
        "packages/python/dirsql/*",
        "packages/python/Cargo.toml",
    ),
    "ts": (
        "packages/ts/src/*",
        "packages/ts/napi/src/*",
        "packages/ts/napi/Cargo.toml",
    ),
}

# `packages/<pkg>/changelog.d/<file>` -- captures the package slug and filename.
_FRAGMENT_PATH_RE = re.compile(r"^packages/([^/]+)/changelog\.d/([^/]+)$")

# A well-formed fragment path: a package changelog dir, then the template-lib
# name (an ISO date, a kebab-case slug, and `.md`). Matched whole so there is
# no basename-splitting to mutate.
_FRAGMENT_NAME_RE = re.compile(
    r"^packages/[^/]+/changelog\.d/\d{4}-\d{2}-\d{2}-[a-z0-9]+(?:-[a-z0-9]+)*\.md$"
)


def package_for_path(path: str) -> str | None:
    """The SDK package a changed path belongs to, or ``None`` if it is not
    public-facing SDK source (docs, tests, CI, tooling)."""
    if any(fnmatch.fnmatch(path, pattern) for pattern in _TEST_EXCLUDES):
        return None
    for package, patterns in _PACKAGE_SOURCES.items():
        if any(fnmatch.fnmatch(path, pattern) for pattern in patterns):
            return package
    return None


def changed_packages(paths) -> set[str]:
    """The set of SDK packages whose source changed among ``paths``."""
    return {
        package
        for package in (package_for_path(path) for path in paths)
        if package is not None
    }


def fragment_package(path: str) -> str | None:
    """The package a changelog fragment path belongs to, or ``None`` when the
    path is not a fragment (wrong location, the dir README, or not ``.md``)."""
    match = _FRAGMENT_PATH_RE.match(path)
    if match is None:
        return None
    package, name = match.group(1), match.group(2)
    if package not in _PACKAGE_SOURCES or name == "README.md" or not name.endswith(".md"):
        return None
    return package


def is_valid_fragment_name(path: str) -> bool:
    """Whether a fragment path's filename matches ``YYYY-MM-DD-<slug>.md``."""
    return _FRAGMENT_NAME_RE.match(path) is not None


def fragment_paths(paths) -> list[str]:
    """The changelog-fragment paths (any package) among ``paths``."""
    return [path for path in paths if fragment_package(path) is not None]


def extract_skip_trailers(trailer_output: str) -> list[str]:
    return [line for line in trailer_output.splitlines() if line.strip()]


def contains_skip_changelog_line(commit_messages: str) -> bool:
    """True if any commit-message line is a ``skip-changelog:`` directive.

    Detects an *attempted* skip-changelog independent of git's trailer parser,
    so the gate can tell a malformed (e.g. blank-line-split) trailer apart from
    no skip at all and emit a targeted fix message.
    """
    return any(
        line.strip().lower().startswith("skip-changelog:")
        for line in commit_messages.splitlines()
    )
