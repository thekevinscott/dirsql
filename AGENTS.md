# dirsql Development

## Cross-Language Parity

dirsql ships SDKs in Rust, Python, and TypeScript. Aim for **complete API parity across all three languages**: same concepts, same capabilities, same naming where possible. Exceptions are allowed for language-idiomatic patterns:

- **Python**: `await db.ready()` (method call, not awaitable property). snake_case. Async iterators for event streams.
- **TypeScript**: `await db.ready` (awaitable property is idiomatic). camelCase. AsyncIterables for event streams.
- **Rust**: Builder pattern or `db.ready().await`. snake_case. Stream trait for event streams.

When adding a feature to one SDK, create beads for the other two. Don't let them drift apart.

## Scratch Files

Write scratch/temporary files to `/tmp` instead of asking permission. Use unique filenames to avoid collisions with other sessions.
Temporary scripts, including Node or shell helpers, must also be written to `/tmp` and executed from there.

## Workflow

- Work in git worktrees under `.worktrees/` folder
- **NEVER commit directly to main** - always create a PR
- One PR per bead. Beads should be concise and small -- as small as possible while still being useful
- Use `bd` (Beads) for task tracking: `bd list`, `bd show <id>`, `bd ready`
- **NEVER inspect or modify `.beads/` directly**. Treat `.beads/` as an internal Beads implementation detail that is off limits. All issue tracking operations must go through the Beads CLI (`bd ...`) only.
- **Bead first**: When starting new work, the first step is always to create a bead (`bd create`). No implementation work begins without a bead.
- These workflow rules apply to **all** changes, including documentation-only changes and updates to `AGENTS.md` or other instruction files. No exceptions.

### Agent Identity and Auth

- Agents must use the approved robot identity for git and GitHub operations. Do **not** use a personal non-robot identity such as `me@thekevinscott.com`.
- Before any `git commit`, `git push`, or `gh pr create`, run `scripts/agent-preflight.sh <commit|push|pr>`.
- The approved robot identity must be provided explicitly via environment variables:
  - `APPROVED_GIT_NAME`
  - `APPROVED_GIT_EMAIL`
  - `AGENT_NAME`
  - `AGENT_MODEL`
- Approved robot credentials and wrappers are allowed. For example, environment sourced from `ROBOT_*` variables is valid for agent operations.
- Prefer the Claude-style wrapper/env model for all git and GitHub operations. Launch the agent through the approved robot wrapper or export equivalent robot environment variables before running `git` or `gh`.
- Do **not** rely on ambient personal shell identity. Do **not** write worktree-local `user.name`, `user.email`, or signing config unless explicitly requested.
- Commits must be GPG-signed with the approved robot signing key and must show as verified on GitHub.
- Configure signing through the approved robot wrapper/env before committing, then verify with `scripts/agent-preflight.sh`.
- If the approved robot identity is not active, stop and ask. Never proceed with a non-robot personal identity.
- Every agent-authored commit message must include this trailer at the bottom: `Agent: <assistant> (<model>)`
- Examples:
  - `Agent: Codex (gpt-5-codex)`
  - `Agent: Claude (Sonnet 4.5)`

### Git Worktrees

**ALL work happens in git worktrees.** Never edit files in the root repo directory. Never commit outside a worktree.

#### Creating a Worktree

```bash
git worktree add .worktrees/my-feature -b feat/my-feature
cd .worktrees/my-feature
```

#### Removing a Worktree

**DANGER: removing a worktree while your shell CWD is inside it permanently breaks the shell.** The ONLY safe procedure:

```bash
# Step 1: Move CWD to the root repo FIRST (not optional)
cd /home/duncan/work/code/projects/dirsql

# Step 2: Now remove the worktree
git worktree remove .worktrees/my-feature
```

**Do NOT skip step 1. Do NOT substitute `git -C` for `cd`.**

### Beads Workflow

**Lifecycle:**
1. **Claim it FIRST**: `bd update <id> --claim` before any work
2. **Create worktree and branch**
3. **Link the PR**: `bd update <id> --external-ref "gh-<pr-number>"` after creating the PR
4. **Close**: `bd close <id>` immediately after the PR is merged

### Subagent Workflow

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

### Orchestrator Responsibilities

The orchestrator (main Claude session) must proactively:
1. **Monitor all open PRs** -- don't wait for the user to report failures. Check CI status after agent completion and on an ongoing basis.
2. **Fix CI failures** on open PRs immediately, either directly or by dispatching a fix agent.
3. **Handle post-merge cleanup** as soon as a PR merges (pull main, remove worktree, delete branch, close bead).
4. **Keep the user informed** of PR status without being asked.
5. **Use foreground monitoring** when waiting on CI and there's no other work to do. Background monitoring causes the conversation to go silent -- use it only when there's genuinely parallel work to perform.
6. **Scripts to `/tmp`**: For polling/monitoring scripts (watching CI, waiting for merges), write the script to `/tmp` then run it via `bash /tmp/script.sh`. Do not use inline bash loops in tool calls.

### Post-Merge Cleanup

After a PR merges, the agent (or orchestrator) must:
1. Pull main in the **root repo**: `git -C /home/duncan/work/code/projects/dirsql pull origin main`
2. **Move CWD to root repo first** (CRITICAL -- never remove a worktree from inside it): `cd /home/duncan/work/code/projects/dirsql`
3. Remove the worktree: `git worktree remove .worktrees/<name>`
4. Delete the local branch: `git branch -d <branch-name>`
5. **Verify the bead is addressed** by the merged PR, then close it: `bd close <id>`

## Testing

### Red/Green Development

Follow **red/green** (test-first) methodology:

1. **Write the test first** -- it must capture the desired behavior
2. **Run it and confirm it fails (RED)** -- do NOT proceed until the test turns red reliably. A test that passes before implementation proves nothing.
3. **Make the minimal change to pass (GREEN)** -- only then write the implementation
4. Refactor if needed, keeping tests green

### TDD Order: Outside-In

Tests are written **before** implementation, starting from the outermost layer:

1. **Integration test first** -- proves the feature works from the consumer's perspective
2. **Unit tests** -- written as you implement each module

A feature is not done until integration tests pass and cover the new functionality.

### When to Write What

**Does the commit change the public-facing API?**
- Yes -> **integration test required**, plus unit tests as you go
- No -> Check if adequate integration coverage already exists:
  - Adequate -> unit tests only
  - Gaps -> add the missing integration tests, plus unit tests

**Always write unit tests.** The question is whether you also need integration tests.

### Test Locations

- **Unit tests**: Colocated with source
  - Python: `foo.py` -> `foo_test.py` in same directory
  - Rust: inline `#[cfg(test)]` module at bottom of each source file
- **Integration tests**: `tests/integration/` -- test the Python SDK layer, mock third-party deps (SQLite, LLM calls). Heavy use of pytest fixtures. Run in CI.
- **E2E tests**: `tests/e2e/` -- real filesystem, real SQLite, real LLM calls, no mocks. Heavy use of pytest fixtures. **NOT run in CI** (eventual LLM calls make them non-free). Run locally by Claude after significant code changes.

### E2E Test Policy

E2E tests are your primary feedback mechanism. Run them liberally after significant changes -- they catch issues that integration tests miss because integration tests mock out SQLite and (eventually) LLM calls. But do NOT add them to CI workflows. They are a local development tool.

See skillet or karat for examples of test organization, fixtures, and pytest-describe patterns.

### Benchmarks

Run `cargo bench -p dirsql-core` after significant changes to the Rust codebase. Not in CI -- local only. Covers: SQLite operations, directory scanning, row diffing, glob matching. Use to catch performance regressions before merging.
