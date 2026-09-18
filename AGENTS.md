# dirsql Development

In your responses, strive for brevity. As concise as possible.

@agents/build/environment.md

## Rules

1. **Never merge a PR.** Merging to `main` publishes a release. Shepherd to green and stop; only the maintainer merges.
2. **Never commit to `main`.** Work in a worktree under `.worktrees/`. One unique branch per PR -- never reuse a name, even for follow-up work on the same issue.
3. **Red/green, CI-confirmed.** Write the failing test first, push it as its own commit, and confirm CI goes red *for the reason that test asserts* before writing any implementation. Removing behavior is exempt.
4. **Run `just preflight` before pushing -- and commit first.** Its diff-scoped gates read `origin/main..HEAD`, so on a dirty tree they examine nothing and report success.
5. **Run the e2e suite locally before pushing a substantial change**, then write the receipt (`just e2e-attest-python` / `just e2e-attest-ts`). CI never runs e2e, only checks the receipt. A `packages/rust/src` change needs both.
6. **Exercise the feature by hand before calling it done.** Build it, run it, confirm the output matches the spec. A green suite does not catch a startup script that errors or a wrong path in a docstring.
7. **Issue first**, then the branch, then `Fixes #<n>` in the PR body. Never request the maintainer's review.
8. **Comments: default to none.** Only a non-obvious *why* -- a constraint, an invariant, a workaround. Never archaeology: no issue references, no "added for the X flow", no restating the code.
9. **Never chain shell commands** with `;`, `&&`, or `||`; it breaks the per-command permission model. Pipes and heredocs are fine. Scratch files go to `/tmp` under unique names. `uv` never `pip`, `pnpm` never `npm`, `trash-put` never `rm`.
10. **Every PR is M or smaller.** Break a larger change into a sequence and stack it, each PR based on the one below, rather than serializing.

Run `scripts/agent-preflight.sh <commit|push|pr>` before every `git commit`, `git push`, and `gh pr create`.

Everything a CI gate enforces is deliberately absent here. Each check names the file and the fix when it fails; read the failure.

## References

Not auto-loaded. **Read the relevant file before working in that area.**

- `ARCHITECTURE.md` -- architecture, cross-language parity, SDK design
- `agents/reference/test-tiers.md` -- unit / integration / binding / e2e / distcheck: where each lives, what each may mock
- `agents/reference/testing-gates.md` -- colocation, coverage floors, mutation, and running them via `just preflight`
- `agents/reference/testing-conventions-ci.md` -- the reusable-workflow gates and their startup failures
- `agents/reference/e2e-attestation.md` -- receipts, per-package scope, the PR body template
- `agents/reference/changelog-migrations.md` -- changelog and migration fragments, the PR body template
- `agents/reference/pr-workflow.md` -- sizing, stacking, releases, merge conflicts, `PARITY.md`, benchmarks
- `agents/reference/pr-monitor.md` -- the `CI Gate` aggregator: reading it, debugging it
- `agents/reference/conventions.md` -- naming, imports, dependency hygiene, where CI logic may live
- `agents/reference/source-size.md` -- judging a change that claims to shrink the codebase
- `agents/reference/session-handoff.md` -- the per-session handoff doc; deliver it at every stopping point
