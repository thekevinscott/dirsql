"""Orchestration for the changelog-gate check (#494/#496).

Per-package, fragments-only since #565: each SDK package whose source a PR
changes must carry a ``packages/<pkg>/changelog.d/YYYY-MM-DD-<slug>.md``
fragment. The ``skip-changelog:`` trailer is the only bypass.
"""

from __future__ import annotations

import sys

from checks.changelog_gate import git_ops
from checks.changelog_gate.decide import (
    changed_packages,
    contains_skip_changelog_line,
    extract_skip_trailers,
    fragment_package,
    fragment_paths,
    is_valid_fragment_name,
)


def _missing_message(packages: list[str]) -> str:
    lines = "\n".join(
        f"    packages/{pkg}/changelog.d/YYYY-MM-DD-<slug>.md" for pkg in packages
    )
    plural = "s" if len(packages) > 1 else ""
    return f"""\
::error file=CHANGELOG.md::SDK code changed but no changelog fragment was added for {len(packages)} package{plural}: {", ".join(packages)}.

Every PR that modifies a package's public-facing SDK source must add a
changelog fragment under that package's own directory:

{lines}

The date is the UTC merge date; the body leads with a Keep a Changelog
category (**Added** / **Changed** / **Deprecated** / **Removed** / **Fixed** /
**Security**). Fragments are permanent -- nothing is assembled back into
CHANGELOG.md.

If the change is also breaking, add a matching migration fragment under
packages/<pkg>/migrations.d/ (see that package's MIGRATIONS.md).

Escape hatch: if this PR genuinely has no observable change (pure refactor,
internal rename, etc.), add a trailer to any commit:

    skip-changelog: <reason>

See AGENTS.md, section "Changelog and Migrations", for the full rule."""


def _malformed_name_message(paths: list[str]) -> str:
    lines = "\n".join(f"    {path}" for path in paths)
    return f"""\
::error file=CHANGELOG.md::A changelog fragment's filename is malformed.

These fragment file(s) are not named `YYYY-MM-DD-<slug>.md` (an ISO merge
date, a kebab-case slug, then `.md`):

{lines}

Rename each to e.g. `2026-07-13-fix-watcher-race.md`. See AGENTS.md,
section "Changelog and Migrations"."""


MALFORMED_SKIP_CHANGELOG_MESSAGE = """\
::error file=CHANGELOG.md::A `skip-changelog:` line is present but git did not parse it as a trailer.

A `skip-changelog:` bypass must be a real git trailer: in the LAST
paragraph of a commit message, with no blank line separating it from the
other trailers (Co-Authored-By:, etc.). As written it sits in its own
paragraph, so git ignores it and this gate sees no bypass.

Fix it either way:
  - reword the commit so `skip-changelog: <reason>` is in the final
    trailer block (no blank line before it), or
  - add a changelog fragment under the changed package's
    packages/<pkg>/changelog.d/ (named YYYY-MM-DD-<slug>.md).

See AGENTS.md, section "Changelog and Migrations"."""


def run(
    base_sha: str,
    head_sha: str,
    *,
    changed_files=git_ops.changed_files,
    skip_trailers=git_ops.skip_trailers,
    commit_messages=git_ops.commit_messages,
) -> int:
    files = changed_files(base_sha, head_sha)

    changed = changed_packages(files)
    if not changed:
        print("No SDK code changes detected. Skipping changelog check.")
        return 0

    trailers = extract_skip_trailers(skip_trailers(base_sha, head_sha))
    if trailers:
        print("skip-changelog trailer present. Bypassing changelog check.")
        print("Reason(s):")
        for reason in trailers:
            print(f"  - {reason}")
        return 0

    fragments = fragment_paths(files)
    malformed = [path for path in fragments if not is_valid_fragment_name(path)]
    if malformed:
        print(_malformed_name_message(malformed), file=sys.stderr)
        return 1

    covered = {fragment_package(path) for path in fragments}
    missing = sorted(changed - covered)
    if not missing:
        print(f"Changelog fragment(s) present for: {', '.join(sorted(changed))}. OK.")
        return 0

    # A changed package has no fragment. Distinguish "no bypass attempted" from
    # "a skip-changelog was written but git didn't parse it as a trailer".
    if contains_skip_changelog_line(commit_messages(base_sha, head_sha)):
        print(MALFORMED_SKIP_CHANGELOG_MESSAGE, file=sys.stderr)
    else:
        print(_missing_message(missing), file=sys.stderr)
    return 1
