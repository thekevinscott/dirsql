# Remote Environment Workflow

These rules apply when the Claude Code session is running in a hosted /
managed-agents / cloud sandbox -- i.e. when `CLAUDE_CODE_REMOTE=true`. They
supplement (not replace) the universal rules in `AGENTS.md` and the existing
`agents/build/environment.md` overrides.

If `CLAUDE_CODE_REMOTE` is unset, ignore this file and follow the local
guidance.

## Issue-and-PR discipline

Every unit of work in a remote session must be traceable end-to-end through
GitHub. The flow is:

1. **Every piece of work has a corresponding GitHub issue.** Before starting
   substantive changes, ensure an issue exists that describes the problem or
   feature. If one does not, open it via `mcp__github__issue_write` against
   the session's declared repository scope. Reference the issue in commit
   messages and the PR body.
2. **Every piece of work ends with a pull request.** Do not leave changes on a
   feature branch without a PR. Open the PR via
   `mcp__github__create_pull_request` once the branch has at least one commit
   and the change is ready for review.
3. **The PR must auto-close the issue.** The PR description must contain a
   GitHub closing keyword pointing at the matching issue, e.g. `Closes #123`,
   `Fixes #123`, or `Resolves #123`. This wires the issue's lifecycle to the
   PR's merge so the issue closes automatically.

One issue, one PR. Do not bundle unrelated work; open a new issue for each
distinct change.

## CI must be green and the PR must be mergeable

A PR is not "done" just because it has been opened. Before reporting work as
complete, the agent must confirm both of the following are true:

1. **All CI checks pass.** Poll the PR's check runs via
   `mcp__github__pull_request_read` (or equivalent) and wait until every
   required check has reported success. If a check fails, diagnose the
   failure, push fixes, and re-poll. Do not declare success while any required
   check is pending or failing. Do not bypass hooks (`--no-verify`) or skip
   checks to force a green result.
2. **The PR is in a mergeable state.** Verify GitHub reports the PR as
   mergeable with no conflicts against the base branch. If conflicts appear,
   resolve them locally on the feature branch (typically by merging or
   rebasing the latest base into the feature branch), push the resolution,
   and re-check. Never resolve conflicts by discarding the base branch's
   changes; investigate any unfamiliar diff before overwriting it.

Only when **both** conditions hold -- CI green and PR mergeable -- is the
remote work complete. Report the PR URL, the linked issue number, and the
final CI status to the user.

## Quick checklist

- [ ] GitHub issue exists describing the work.
- [ ] Branch is a child of the assigned session branch (per
      `agents/build/environment.md`), not `main`.
- [ ] Commits reference the issue (`owner/repo#<num>`).
- [ ] PR opened via `mcp__github__create_pull_request`.
- [ ] PR body contains a `Closes #<num>` (or `Fixes` / `Resolves`) line for the
      tracking issue.
- [ ] All required CI checks are green.
- [ ] PR reports as mergeable with no conflicts.
