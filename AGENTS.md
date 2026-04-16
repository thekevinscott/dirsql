# dirsql Development

## Architecture

All architectural decisions and constraints (including cross-language parity rules, the one-implementation principle, and SDK design) are in `ARCHITECTURE.md`. Do NOT put architectural information in this file -- AGENTS.md is for workflow and process only.

@agents/build/environment.md

## Scratch Files

Write scratch/temporary files to `/tmp` instead of asking permission. Use unique filenames to avoid collisions with other sessions.
Temporary scripts, including Node or shell helpers, must also be written to `/tmp` and executed from there.

## Shell Commands

**Do not chain commands** with `;`, `&&`, or `||`. Chained commands break the per-command permission model -- each command must be evaluated separately, and chaining forces a single bulk approval (or prompt) for the whole pipeline. Run each command as its own call.

Exceptions: piping (`|`) is fine when it's genuinely one logical operation (e.g., `cmd | jq`). Heredocs (`cat <<EOF`) are fine. `cd path && cmd` is NOT fine -- use `cd` as a separate call (or pass absolute paths).

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

### E2E Before Push

Agents must run the full e2e suite locally before any `git push` that includes a **substantial code change**, and report the outcome in the PR body. The commands to run differ per environment -- see the active environment file for specifics.

**"Substantial" means any change touching:**
- `packages/rust/**` (Rust core)
- `packages/python/src/**` (excluding files matching `*_test.py`)
- `packages/ts/src/**` (excluding files matching `*.test.ts` / `*.spec.ts`)
- Any shared SDK runtime code reachable from the above

**Not substantial** (e2e is optional, note "N/A - docs/lint/typo only" in the PR body):
- Docs (`*.md`, `docs/**`, `README*`)
- Lint/format-only changes
- Typo fixes with no behavior change
- Test-only changes (test files themselves)
- CI/workflow config

**PR body requirement:** PRs that include substantial changes must contain this section verbatim (checkboxes filled in):

```markdown
## E2E Verification

- [ ] Ran e2e suites locally for every affected SDK
- [ ] Python SDK e2e: pass / fail / N/A
- [ ] TypeScript SDK e2e: pass / fail / N/A
- [ ] Rust core e2e (if applicable): pass / fail / N/A
- Command(s) run:
- Result summary:
```

For docs/lint/typo-only PRs, include the section with a single line: `N/A - docs/lint/typo only`.

### Docs as Spec

**Docs are the canonical specification.** Every documented feature must have a corresponding test. Every test must trace back to a documented feature. If it's not in the docs, don't test it (and question whether it should exist). If it's in the docs, it must have a test.

When adding a feature, the PR must include docs AND tests. When docs change, tests update to match. Agents must run e2e tests locally before pushing substantial changes.

### Cross-SDK Parity (PARITY.md)

`PARITY.md` is a **living document** that tracks API-surface parity across the Python, Rust, and TypeScript SDKs. It must stay current.

On every PR that touches any SDK's public API (`packages/python/python/dirsql/`, `packages/rust/src/`, `packages/ts/ts/` or `packages/ts/src/`), the author must:

1. Update `PARITY.md` to reflect the new/changed/removed API surface.
2. Call out in the PR body whether the change is **introducing parity drift** (one SDK gets something the others don't yet) or **restoring parity** (bringing a lagging SDK in line). Drift is allowed but must be intentional and tracked.
3. If drift is introduced, open a follow-up bead for each lagging SDK so the gap is visible.

Orchestrators must block merges of SDK-touching PRs that don't update `PARITY.md`.

### Benchmarks

Run `cargo bench -p dirsql` after significant changes to the Rust codebase. Not in CI -- local only. Covers: SQLite operations, directory scanning, row diffing, glob matching. Use to catch performance regressions before merging.

### Coverage Floor

Coverage enforcement must stay explicit in CI for each SDK package:

- Rust core coverage must stay at 90% or higher.
- Python SDK coverage must stay at 90% or higher.
- TypeScript SDK coverage must stay at 90% or higher.

When work affects more than one SDK package, split the coverage and test work across subagents so each package can be validated independently.
