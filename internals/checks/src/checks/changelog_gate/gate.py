"""Orchestration for the changelog-gate check (#494/#496).

Ported from `.github/workflows/changelog-check.yml`'s inline bash -- same messages, same
`::error::` annotations, same exit codes, just testable and driven through injected collaborators.
"""
from __future__ import annotations

import sys

from checks.changelog_gate import git_ops
from checks.changelog_gate.decide import (
    any_sdk_code_changed,
    changelog_fragments,
    contains_skip_changelog_line,
    count_added_lines,
    extract_skip_trailers,
)

MISSING_CHANGELOG_MESSAGE = """\
::error file=CHANGELOG.md::SDK code changed but no changelog entry was added.

Every PR that modifies public-facing SDK code must add a changelog
fragment: a new file changelog.d/<branch-slug>.<category>.md whose
category is one of added/changed/deprecated/removed/fixed/security,
containing the entry body. (Editing CHANGELOG.md under '## [Unreleased]'
directly is still accepted, but conflicts with other in-flight PRs.)
If the change is also breaking, behavior-altering, or removes a
deprecated symbol, add a matching migration entry as
migrations.d/<branch-slug>.md (see MIGRATIONS.md's template).

Escape hatch: if this PR genuinely has no observable change (pure
refactor, internal rename, etc.), add a trailer to any commit:

    skip-changelog: <reason>

See AGENTS.md, section "Changelog and Migrations", for the full rule."""

NO_ADDED_CONTENT_MESSAGE = (
    "::error file=CHANGELOG.md::CHANGELOG.md was touched but has no added content."
)

MALFORMED_SKIP_CHANGELOG_MESSAGE = """\
::error file=CHANGELOG.md::A `skip-changelog:` line is present but git did not parse it as a trailer.

A `skip-changelog:` bypass must be a real git trailer: in the LAST
paragraph of a commit message, with no blank line separating it from the
other trailers (Co-Authored-By:, etc.). As written it sits in its own
paragraph, so git ignores it and this gate sees no bypass.

Fix it either way:
  - reword the commit so `skip-changelog: <reason>` is in the final
    trailer block (no blank line before it), or
  - add a changelog fragment: changelog.d/<branch-slug>.<category>.md.

See AGENTS.md, section "Changelog and Migrations"."""


def run(
    base_sha: str,
    head_sha: str,
    *,
    changed_files=git_ops.changed_files,
    skip_trailers=git_ops.skip_trailers,
    changelog_diff=git_ops.changelog_diff,
    commit_messages=git_ops.commit_messages,
) -> int:
    files = changed_files(base_sha, head_sha)

    if not any_sdk_code_changed(files):
        print("No SDK code changes detected. Skipping changelog check.")
        return 0

    trailers = extract_skip_trailers(skip_trailers(base_sha, head_sha))
    if trailers:
        print("skip-changelog trailer present. Bypassing CHANGELOG check.")
        print("Reason(s):")
        for reason in trailers:
            print(f"  - {reason}")
        return 0

    fragments = changelog_fragments(files)
    if fragments:
        print(f"Changelog fragment present: {', '.join(fragments)}. OK.")
        return 0

    if "CHANGELOG.md" not in files:
        # Distinguish "no bypass attempted" from "a skip-changelog was written
        # but git didn't parse it as a trailer" -- the latter needs a targeted
        # fix, not the generic "add an entry" message.
        if contains_skip_changelog_line(commit_messages(base_sha, head_sha)):
            print(MALFORMED_SKIP_CHANGELOG_MESSAGE, file=sys.stderr)
        else:
            print(MISSING_CHANGELOG_MESSAGE, file=sys.stderr)
        return 1

    added = count_added_lines(changelog_diff(base_sha, head_sha))
    if added < 1:
        print(NO_ADDED_CONTENT_MESSAGE, file=sys.stderr)
        return 1

    print(f"CHANGELOG.md updated with {added} added line(s). OK.")
    return 0
