# Remote Execution Environment (hosted Claude Code session)

This file provides the workflow rules that apply when the session is running in a **hosted Claude Code sandbox** (cloud / web). It is loaded via the `agents/build/environment.md` symlink, which is (re)created on every session start by `.claude/hooks/select-environment.sh` based on the `CLAUDE_CODE_REMOTE` env var.

Universal rules (architecture, scratch files, shell command style, testing philosophy) live in `AGENTS.md`. This file covers only the **remote-specific overrides** -- what changes when there is no `~/work/dotfiles`, no GPG keyring, no `gh`, no `just`, and no local LLM credentials.

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

## Task tracking

Track work via GitHub issues, using the `mcp__github__*` tools (the sandbox restricts `gh`, so the local environment's `gh issue ...` commands do not apply here). Reference issues by `owner/repo#<num>` in commit messages and PR bodies; use `Fixes #<num>` where appropriate.

## Permissions and tool access

The sandbox restricts `gh` CLI access. **All GitHub operations must go through `mcp__github__*` MCP tools.** Repository scope is limited to whatever the session declares; do not attempt operations against other repositories.

## Path assumptions

Do **NOT** hardcode `/home/duncan/...`. Use `$PWD` or the actual sandbox root (e.g. `/home/user/dirsql`).

## Testing commands

`just` is typically not available. Substitute the underlying commands:

- Python: run `pytest` directly against `packages/python`.
- TypeScript: `pnpm --dir packages/ts run <script>`.
- Rust: `cargo test --workspace --features cli` (plain `cargo test --workspace` skips the feature-gated CLI e2e tests); `cargo bench -p dirsql` for benches.

The testing-conventions gates are the trap here: `pip install testing-conventions` cannot build in the sandbox, so `uvx testing-conventions <gate>` is the natural reach -- and for the **python mutation** gate it is wrong. `uvx` runs the tool from its own environment, so cosmic-ray's `python3 -m pytest` resolves to an interpreter with no pytest instead of `packages/python/.venv`, and the gate dies on a baseline failure that names no cause (#706). Run that one through the project venv instead:

```bash
cd packages/python && uv run --with testing-conventions testing-conventions unit mutation --language python --base origin/main --config testing-conventions.toml dirsql
```

`uvx` is fine for the gates that do not execute the python suite (`unit colocated-test`, `unit lint`, `e2e verify`, `e2e attest`). Full detail: `agents/reference/testing-gates.md`.

## E2E suites

E2E suites that make live LLM calls cannot run in the hosted sandbox. In the PR body's `## E2E Verification` section, state this explicitly (e.g. `blocked-remote: no LLM credentials in sandbox`) instead of claiming pass/fail.

The python and TS `tests/e2e` suites have no LLM dependency, so they run here. When a PR changes a package, refresh that package's e2e attestation in the sandbox (see AGENTS.md, "E2E Attestation"). `pip install testing-conventions` fails to build in the sandbox and `just` is absent, so use `uvx` and the recipe bodies directly. **Keep the `>=0.0.91` floor** -- older releases prune every other branch's receipt and commit the deletions (`agents/reference/e2e-attestation.md`):

```bash
cd packages/python && uvx --from 'testing-conventions>=0.0.91' testing-conventions e2e attest 'uv run python -m pytest tests/e2e/ -x -q'   # = just test-e2e
cd packages/ts && uvx --from 'testing-conventions>=0.0.91' testing-conventions e2e attest 'pnpm test:e2e'
```

Attest each package as the last commit touching it. Some e2e suites need a prior build (the Rust binary / native ext); build first if a run reports a missing artifact. Only suites that later grow live-LLM cases stay blocked here; note those in the PR body.

CI on GitHub remains the authoritative gate; the orchestrator continues to monitor it via `mcp__github__*` tools.

## Subagent / Orchestrator adjustments

The subagent and orchestrator responsibilities from the local environment still apply, with these changes:

- Skip `scripts/agent-preflight.sh`.
- Skip worktree creation -- use child branches off the session branch instead.
- Use `mcp__github__*` for every GitHub operation (status checks, PR creation, comments, merges) rather than `gh`.
- Orchestrator still monitors CI, fixes failures, and keeps the user informed, but via MCP tools only.

## Post-merge cleanup

There is no worktree to remove. Cleanup reduces to:

1. Pull `main` into the sandbox checkout: `git pull origin main`.
2. Delete the merged feature branch locally: `git branch -d <branch-name>`.

Do **NOT** try to `git worktree remove`. The issue closes itself via the PR's `Fixes #<num>`.
