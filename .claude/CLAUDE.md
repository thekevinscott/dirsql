# dirsql Development

## Scratch Files

Write scratch/temporary files to `/tmp` instead of asking permission. Use unique filenames to avoid collisions with other sessions.

## Workflow

- Work in git worktrees under `.worktrees/` folder
- **NEVER commit directly to main** - always create a PR
- One PR per bead. Beads should be concise and small -- as small as possible while still being useful
- Use `bd` (Beads) for task tracking: `bd list`, `bd show <id>`, `bd ready`

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
1. Creates a worktree and branch for its bead
2. Does the implementation work (red/green TDD)
3. Pushes the branch and opens a PR
4. Monitors the PR and proactively resolves:
   - CI failures
   - GPG signing complaints
   - Merge conflicts
5. Continues monitoring until the PR is in a mergeable state

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
