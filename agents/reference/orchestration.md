# Orchestration

For the session that dispatches agents and shepherds their PRs. A single-PR agent doesn't need this file.

## Main never goes red

If any PR, test, or agent report implies something fails on `main`, stop. Flag it to the maintainer before it can merge, and recommend holding it. A red `main` blocks every PR that touches package code, because no receipt can pass, and recovering costs hours of local e2e runs. Also look for the gate hole that let a red change look green.

## Briefs

Put these rules into every agent brief:

- **Base `main`.** No stacked PRs.
- **Merge `main` into a branch only to resolve a conflict.** `main` doesn't require up-to-date branches, and every new head reruns CI. That includes mutation, which takes an hour on a large PR.
- **Never run mutation locally.** That includes plain `just preflight`.
- **Never merge.**
- **Batch fixes into one push.**

Scope each task on its merits. Never shrink it, or plan early stopping points, around usage limits; that budget call is the maintainer's.

## Targeted mutation recheck

The orchestrator only may make one exception to "no local mutation". After CI reports survivors and a fix is ready, check just the lines changed since that CI run, before pushing:

```bash
git diff <ci-sha> <head> -- packages/rust/src > /tmp/<unique>.diff
systemd-run --user --scope -p MemoryMax=24G -p MemorySwapMax=0 \
  cargo mutants --in-diff /tmp/<unique>.diff -p dirsql --dir <scratch worktree> -j 2 -- --lib -- --skip watch --skip poll
```

- One run machine-wide, never the full PR diff, and never inside a dispatched agent.
- `--skip watch --skip poll` is there because the host's inotify limit fails the watcher tests in the unmutated baseline.
- Push only once nothing survives.

## Review every returned PR for comment bloat

Before reporting a PR ready, read its added comments (`gh pr diff <n>`) and trim them yourself. Keep only a comment that says something the code can't: a constraint, an invariant, or a workaround. Rationale for the change belongs in the PR body.

## Issues

When burning down issues, pick blocker-free tickets and clear bugs. Leave the maintainer's one-line idea issues alone unless asked for a spec.

## Talking to the maintainer

Name every referent, as in "the drafted embed-protocol issue (batching and wire format)". Never "Issue 1": numbers collide across parallel sessions. Present product decisions as options plus one recommendation, then stop.
