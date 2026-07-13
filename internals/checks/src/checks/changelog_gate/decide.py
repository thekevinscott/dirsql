"""Pure decision logic for the changelog-gate check (#494/#496).

Mirrors template-lib's reference gate (#566) in structure, adapted to dirsql's
**per-package colocation**: fragments live inside the package they document --
``packages/<pkg>/changelog.d/`` and ``packages/<pkg>/migrations.d/`` -- so they
ship with that package. A PR that changes non-test source under
``packages/<pkg>/`` must add a fragment under that package's own fragment
folders, named ``YYYY-MM-DD-<slug>.md`` (the directory identifies the package,
so no package token in the filename). The ``skip-changelog:`` commit-body line
is the bypass.
"""

import re

# A `skip-changelog:` line anywhere in a commit body bypasses the gate. Scanned
# over raw bodies (not git's trailer parser), so it need not be a formal
# trailer -- this sidesteps the blank-line-splits-the-trailer footgun.
_SKIP_TRAILER = re.compile(r"^skip-changelog:", re.IGNORECASE | re.MULTILINE)

# A fragment sits directly inside a package's changelog.d/ or migrations.d/.
# Captures (<pkg>, <filename>); the trailing segment forbids nested paths.
_FRAGMENT_RE = re.compile(r"packages/([^/]+)/(?:changelog|migrations)\.d/([^/]+)")

# Template-lib fragment name: an ISO date, a kebab-case slug, and `.md`.
_FRAGMENT_NAME = re.compile(r"\d{4}-\d{2}-\d{2}-[a-z0-9-]+\.md")


def has_skip_trailer(commit_messages: str) -> bool:
    """True if any commit-body line starts with ``skip-changelog:``."""
    return bool(_SKIP_TRAILER.search(commit_messages))


def changed_packages(changed) -> list[str]:
    """Unique, sorted package names touched under ``packages/<name>/...``."""
    pkgs = {
        parts[1]
        for path in changed
        if len(parts := path.split("/")) >= 2 and parts[0] == "packages"
    }
    return sorted(pkgs)


def _is_exempt(path: str, pkg: str) -> bool:
    """True if ``path`` is a fragment, stub, or test file -- not source.

    The package ``CHANGELOG.md`` / ``MIGRATIONS.md`` are pointer stubs; the
    ``changelog.d/`` / ``migrations.d/`` folders are the entries themselves;
    dirsql colocates Python unit tests as ``*_test.py`` and TS as ``*.test.*``
    / ``*.spec.*``; and anything under a ``tests/`` directory is a test tier.
    """
    p = re.escape(pkg)
    return bool(
        re.fullmatch(rf"packages/{p}/(CHANGELOG|MIGRATIONS)\.md", path)
        or re.match(rf"packages/{p}/(changelog|migrations)\.d/", path)
        or re.fullmatch(rf"packages/{p}/.*_test\.py", path)
        or re.fullmatch(rf"packages/{p}/.*\.(test|spec)\.(ts|tsx|js|mjs|cjs)", path)
        or re.match(rf"packages/{p}/tests?/", path)
    )


def code_touched(changed, pkg: str) -> bool:
    """True if the package has non-stub, non-test source changes."""
    prefix = f"packages/{pkg}/"
    return any(
        path.startswith(prefix) and not _is_exempt(path, pkg) for path in changed
    )


def _fragment(path: str):
    """``(<pkg>, <filename>)`` if ``path`` sits directly in a package's
    changelog.d/ or migrations.d/, else ``None``."""
    match = _FRAGMENT_RE.fullmatch(path)
    return (match.group(1), match.group(2)) if match is not None else None


def malformed_fragments(changed) -> list[str]:
    """Touched fragment files whose names break the naming convention."""
    return [
        path
        for path in changed
        if (frag := _fragment(path)) is not None
        and frag[1] != "README.md"
        and not _FRAGMENT_NAME.fullmatch(frag[1])
    ]


def added_fragments(added, pkg: str) -> list[str]:
    """Well-formed fragments the PR adds under ``pkg``'s own fragment dirs."""
    return [
        path
        for path in added
        if (frag := _fragment(path)) is not None
        and frag[0] == pkg
        and _FRAGMENT_NAME.fullmatch(frag[1])
    ]
