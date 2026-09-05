"""Orchestration for the changelog-gate check (#494/#496).

Mirrors template-lib's reference gate (#566), adapted to dirsql's per-package
colocation: a PR that changes non-test source under ``packages/<pkg>/`` or
``plugins/<pkg>/`` must add a fragment under that same package's
``changelog.d/`` (or ``migrations.d/``), named ``YYYY-MM-DD-<slug>.md``. A
``skip-changelog:`` commit-body line bypasses the whole gate. The git plumbing
is injected so the orchestration unit-tests without a real repo.
"""

from __future__ import annotations

from checks.changelog_gate.added_files import added_files as _added_files
from checks.changelog_gate.changed_files import changed_files as _changed_files
from checks.changelog_gate.commit_messages import commit_messages as _commit_messages
from checks.changelog_gate.added_fragments import added_fragments
from checks.changelog_gate.code_touched import code_touched
from checks.changelog_gate.decide import changed_packages, has_skip_trailer
from checks.changelog_gate.malformed_fragments import malformed_fragments


def run(
    base_sha: str,
    head_sha: str,
    *,
    changed_files=_changed_files,
    added_files=_added_files,
    commit_messages=_commit_messages,
) -> int:
    if has_skip_trailer(commit_messages(base_sha, head_sha)):
        print("skip-changelog line present; bypassing changelog enforcement.")
        return 0

    changed = changed_files(base_sha, head_sha)
    added = added_files(base_sha, head_sha)

    fail = 0
    for path in malformed_fragments(changed):
        print(
            f"::error file={path}::fragment filenames must match "
            f"YYYY-MM-DD-<slug>.md (UTC merge date; lowercase letters, digits, "
            f"hyphens). See AGENTS.md, 'Changelog and Migrations'."
        )
        fail = 1

    packages = changed_packages(changed)
    if not packages and not fail:
        print("No package source changed; nothing to enforce.")
        return 0

    for pkg in packages:
        if not code_touched(changed, pkg):
            continue
        if added_fragments(added, pkg):
            continue
        print(
            f"::error::{pkg} has code changes but no changelog fragment "
            f"was added. Add {pkg}/changelog.d/YYYY-MM-DD-<slug>.md (plus "
            f"a {pkg}/migrations.d/ fragment if the change is breaking), "
            f"or add a 'skip-changelog: <reason>' line to any commit for a "
            f"genuinely internal refactor. See AGENTS.md, 'Changelog and Migrations'."
        )
        fail = 1
    return fail
