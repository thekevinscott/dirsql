# dirsql Development

In your responses, strive for brevity. As concise as possible.

@agents/build/environment.md

@agents/reference/session-handoff.md

## Rules

1. **Never merge a PR.** Merging to `main` publishes a release. Shepherd to green and stop; only the maintainer merges.
2. **Never commit to `main`.** Work in a worktree under `.worktrees/`. One unique branch per PR, prefixed `bot/` -- never reuse a name, even for follow-up work on the same issue. Merge `main` into a PR branch only to resolve a conflict; every new head reruns CI.
3. **Red/green, CI-confirmed.** Write the failing test first, push it as its own commit, and confirm CI goes red *for the reason that test asserts* before writing any implementation. The test and its fix ship in the same PR, so `main` never goes red. Removing behavior is exempt.
4. **Run preflight without mutation before pushing -- and commit first:** `just preflight --gate colocated-test --gate one-function-per-file --gate unit-lint --gate integration-lint --gate unit-coverage --gate packaging --gate e2e-verify`. Its diff-scoped gates read `origin/main..HEAD`, so on a dirty tree they examine nothing and report success. Never run mutation testing locally -- not plain `just preflight`, `cargo mutants`, or `testing-conventions unit mutation`; parallel runs exhaust the host's memory. CI runs it; fix survivors from the CI log.
5. **Run the e2e tests your change can affect before pushing a substantial change**, then write the receipt. You judge which tests those are; `agents/reference/e2e-attestation.md` has the commands. CI never runs e2e, only checks the receipt. A `packages/rust/src` change needs both packages' receipts.
6. **Exercise the feature by hand before calling it done.** Build it, run it, confirm the output matches the spec. A green suite does not catch a startup script that errors or a wrong path in a docstring.
7. **Issue first**, then the branch, then `Fixes #<n>` in the PR body. Never request the maintainer's review.
8. **Comments: default to none.** Only a non-obvious *why* -- a constraint, an invariant, a workaround. Never archaeology: no issue references, no "added for the X flow", no restating the code.
9. **Never chain shell commands** with `;`, `&&`, or `||`; it breaks the per-command permission model. Pipes and heredocs are fine. Scratch files go to `/tmp` under unique names. `uv` never `pip`, `pnpm` never `npm`, `trash-put` never `rm`.
10. **Every PR is M or smaller.** Break a larger change into a sequence of PRs, each targeting `main`; branch the next one from fresh `main` after the previous merges.
11. **Print the handoff doc's path every time you update it, and at least every few replies.** It changes all session; the latest path must always be near the bottom of the conversation, or a stale resume costs the whole history. See `agents/reference/session-handoff.md`.
12. **Never skip a failing test to reach green** -- no `#[ignore]`, xfail, quarantine, or `continue-on-error`. Fix the bug. CI is the only proof of Windows support.
13. **No Claude attribution** in commits, PR bodies, or branch names: no `Co-Authored-By` trailer, session link, or "Generated with" footer. The signed robot identity is the provenance.
14. **Never pin or floor a tool version to dodge a bug.** Upgrade the local install, or fix the tool upstream.
15. **Before designing a feature, read `agents/reference/product-principles.md`.** Its decisions include: read-only, the CLI first, globs follow bash, and YAGNI.

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
- `agents/reference/pr-workflow.md` -- sizing, releases, merge conflicts, `PARITY.md`, benchmarks
- `agents/reference/pr-monitor.md` -- the `CI Gate` aggregator: reading it, debugging it
- `agents/reference/conventions.md` -- naming, imports, dependency hygiene, where CI logic may live
- `agents/reference/source-size.md` -- judging a change that claims to shrink the codebase
- `agents/reference/product-principles.md` -- maintainer decisions that bound any design
- `agents/reference/orchestration.md` -- for a session that dispatches agents: briefs, a red `main`, the targeted mutation recheck, PR review
