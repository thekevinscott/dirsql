# Local Execution Environment

This file provides the workflow rules that apply when developing **locally** on a maintainer machine. It is loaded via the `agents/build/environment.md` symlink, which is (re)created on every session start by `.claude/hooks/select-environment.sh`.

Universal rules (architecture, scratch files, shell command style, testing philosophy) live in `AGENTS.md`. This file covers only the **local-specific overrides**.

## Workflow

- Work in git worktrees under `.worktrees/` folder.
- **NEVER commit directly to main** -- always create a PR.
- One PR per bead. Beads should be concise and small -- as small as possible while still being useful.
- Use `bd` (Beads) for task tracking: `bd list`, `bd show <id>`, `bd ready`.
- **NEVER inspect or modify `.beads/` directly.** Treat `.beads/` as an internal Beads implementation detail that is off limits. All issue tracking operations must go through the Beads CLI (`bd ...`) only.
- **Bead first**: When starting new work, the first step is always to create a bead (`bd create`). No implementation work begins without a bead.
- These workflow rules apply to **all** changes, including documentation-only changes and updates to `AGENTS.md` or other instruction files. No exceptions.

## Agent Identity and Auth

- Agents must use the approved robot identity for git and GitHub operations. Do **not** use a personal non-robot identity such as `me@thekevinscott.com`.
- Before any `git commit`, `git push`, or `gh pr create`, run `scripts/agent-preflight.sh <commit|push|pr>`.
- The approved robot identity must be provided explicitly via environment variables. The preflight script accepts either naming convention:
  - Explicit: `APPROVED_GIT_NAME`, `APPROVED_GIT_EMAIL`, `APPROVED_GPG_KEY`
  - Wrapper-style (`ROBOT_*`): `ROBOT_GIT_NAME`, `ROBOT_GIT_EMAIL`, `ROBOT_GPG_KEY_ID`
- The `cc`/`cx` wrappers in `~/work/dotfiles` set the `ROBOT_*` git/GPG vars.
- Prefer the Claude-style wrapper/env model for all git and GitHub operations. Launch the agent through the approved robot wrapper or export equivalent robot environment variables before running `git` or `gh`.
- Do **not** rely on ambient personal shell identity. Do **not** write worktree-local `user.name`, `user.email`, or signing config unless explicitly requested.
- Commits must be GPG-signed with the approved robot signing key and must show as verified on GitHub.
- Configure signing through the approved robot wrapper/env before committing, then verify with `scripts/agent-preflight.sh`.
- If the approved robot identity is not active, stop and ask. Never proceed with a non-robot personal identity.
- Provenance comes from the verified robot git author + GPG signature. No assistant/model trailer is required.

## Git Worktrees

**ALL work happens in git worktrees.** Never edit files in the root repo directory. Never commit outside a worktree.

### Creating a Worktree

```bash
git worktree add .worktrees/my-feature -b feat/my-feature
cd .worktrees/my-feature
```

### Removing a Worktree

**DANGER: removing a worktree while your shell CWD is inside it permanently breaks the shell.** The ONLY safe procedure:

```bash
# Step 1: Move CWD to the root repo FIRST (not optional)
cd /home/duncan/work/code/projects/dirsql

# Step 2: Now remove the worktree
git worktree remove .worktrees/my-feature
```

**Do NOT skip step 1. Do NOT substitute `git -C` for `cd`.**

## Beads Workflow

**Lifecycle:**
1. **Claim it FIRST**: `bd update <id> --claim` before any work
2. **Create worktree and branch**
3. **Link the PR**: `bd update <id> --external-ref "gh-<pr-number>"` after creating the PR
4. **Close**: `bd close <id>` immediately after the PR is merged

## Subagent Workflow

New work on beads should be done via subagents in isolated worktrees. Each subagent:
1. Claims the bead (`bd update <id> --claim`) before starting any work
2. Creates a worktree and branch for its bead
3. Does the implementation work (red/green TDD)
4. Pushes the branch and opens a PR
5. Monitors the PR and proactively resolves:
   - CI failures
   - GPG signing complaints
   - Merge conflicts
6. Continues monitoring until the PR is in a mergeable state
7. When a bead spans multiple SDKs or package lanes, split it into separate subagents and isolated worktrees rather than serially implementing everything in one checkout.
8. **Run e2e tests locally before `git push` on substantial changes** (see "E2E Before Push" below). Report the result in the PR body using the `## E2E Verification` template.

## Orchestrator Responsibilities

The orchestrator (main Claude session) must proactively:
1. **Monitor all open PRs** -- don't wait for the user to report failures. Check CI status after agent completion and on an ongoing basis.
2. **Fix CI failures** on open PRs immediately, either directly or by dispatching a fix agent.
3. **Handle post-merge cleanup** as soon as a PR merges (pull main, remove worktree, delete branch, close bead).
4. **Keep the user informed** of PR status without being asked.
5. **Use foreground monitoring** when waiting on CI and there's no other work to do. Background monitoring causes the conversation to go silent -- use it only when there's genuinely parallel work to perform.
6. **Scripts to `/tmp`**: For polling/monitoring scripts (watching CI, waiting for merges), write the script to `/tmp` then run it via `bash /tmp/script.sh`. Do not use inline bash loops in tool calls.
7. **No permission loops**: If a repo-authorized command needs sandbox escalation, state the exact command and why once, then keep working. Do not ask the user to approve it as a separate yes/no step.
8. **Enforce the E2E-before-push rule**: Before merging any PR that touches substantial code (see "E2E Before Push" below), confirm the PR body contains a completed `## E2E Verification` section. If it's missing, dispatch an agent to run e2e and update the PR body before merge. Do not add e2e to CI -- it stays local-only per the E2E Test Policy.

## E2E Before Push -- Local Commands

The "substantial change" definition and PR body template live in `AGENTS.md`. In the local environment, run e2e via:

- Python SDK: `just test-e2e`
- TypeScript SDK: `pnpm --dir packages/ts run test:e2e` (when a TS e2e target exists)
- Rust core: covered by `cargo test --workspace`; run `cargo bench -p dirsql-core` after Rust-heavy changes

Record the exact commands run and their outcomes in the PR body.

## Post-Merge Cleanup

After a PR merges, the agent (or orchestrator) must:
1. Pull main in the **root repo**: `git -C /home/duncan/work/code/projects/dirsql pull origin main`
2. **Move CWD to root repo first** (CRITICAL -- never remove a worktree from inside it): `cd /home/duncan/work/code/projects/dirsql`
3. Remove the worktree: `git worktree remove .worktrees/<name>`
4. Delete the local branch: `git branch -d <branch-name>`
5. **Verify the bead is addressed** by the merged PR, then close it: `bd close <id>`
