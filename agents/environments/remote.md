# Remote Execution Environment (hosted Claude Code session)

This file provides the workflow rules that apply when the session is running in a **hosted Claude Code sandbox** (cloud / web). It is loaded via the `agents/build/environment.md` symlink, which is (re)created on every session start by `.claude/hooks/select-environment.sh` based on the `CLAUDE_CODE_REMOTE` env var.

Universal rules (architecture, scratch files, shell command style, testing philosophy) live in `AGENTS.md`. This file covers only the **remote-specific overrides** -- what changes when there is no `~/work/dotfiles`, no GPG keyring, no `bd`, no `just`, and no local LLM credentials.

If an item listed here turns out to be available in the sandbox, prefer the corresponding local instruction in `agents/environments/local.md`.

## Identity and signing

The harness has already set a non-personal git identity (e.g. `Claude <noreply@anthropic.com>`). Use it as-is -- do **NOT** rewrite `user.name` / `user.email` and do **NOT** attempt to configure GPG.

The "commits must be GPG-signed by the approved robot key" rule and the `scripts/agent-preflight.sh` gate are both **suspended** in this environment. Commits will be unsigned. Do **not** invoke `scripts/agent-preflight.sh` here.

## Branches and worktrees

The harness assigns a session branch (typically `claude/general-session-<id>`). Use it as the base for all work. Do **NOT** create `.worktrees/` -- the sandbox is already an isolated checkout.

Instead, create one child branch per PR directly from the session branch:

```bash
git checkout -b claude/general-session-<id>-<slug> claude/general-session-<id>
```

One PR per change still applies. The "never commit directly to `main`" rule still applies.

## Task tracking (no Beads)

`bd` is **not** installed. Skip every `bd create` / `bd update --claim` / `bd close` step. Do **NOT** fabricate bead IDs in commits or PR bodies.

Track work via GitHub issues directly, using the `mcp__github__*` tools. Reference issues by `owner/repo#<num>` in commit messages and PR bodies; use `Fixes #<num>` where appropriate.

## Permissions and tool access

The sandbox restricts `gh` CLI access. **All GitHub operations must go through `mcp__github__*` MCP tools.** Repository scope is limited to whatever the session declares; do not attempt operations against other repositories.

## Path assumptions

Do **NOT** hardcode `/home/duncan/...`. Use `$PWD` or the actual sandbox root (e.g. `/home/user/dirsql`).

## Testing commands

`just` is typically not available. Substitute the underlying commands:

- Python: run `pytest` directly against `packages/python`.
- TypeScript: `pnpm --dir packages/ts run <script>`.
- Rust: `cargo test --workspace`; `cargo bench -p dirsql` for benches.

## E2E suites

E2E suites that make live LLM calls cannot run in the hosted sandbox. In the PR body's `## E2E Verification` section, state this explicitly (e.g. `blocked-remote: no LLM credentials in sandbox`) instead of claiming pass/fail.

CI on GitHub remains the authoritative gate; the orchestrator continues to monitor it via `mcp__github__*` tools.

## Subagent / Orchestrator adjustments

The subagent and orchestrator responsibilities from the local environment still apply, with these changes:

- Skip all `bd` commands.
- Skip `scripts/agent-preflight.sh`.
- Skip worktree creation -- use child branches off the session branch instead.
- Use `mcp__github__*` for every GitHub operation (status checks, PR creation, comments, merges) rather than `gh`.
- Orchestrator still monitors CI, fixes failures, and keeps the user informed, but via MCP tools only.

## Post-merge cleanup

There is no worktree to remove. Cleanup reduces to:

1. Pull `main` into the sandbox checkout: `git pull origin main`.
2. Delete the merged feature branch locally: `git branch -d <branch-name>`.

Do **NOT** try to `git worktree remove`. There is no bead to close.
