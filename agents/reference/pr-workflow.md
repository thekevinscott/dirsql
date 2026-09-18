# PR and Issue Workflow

Sizing, stacking, merge gating, and the per-PR obligations that are not enforced by a gate.

## Releases: Merging to Main Publishes

**Merging to `main` is the release trigger — there is no separate release step.** dirsql publishes via [putitoutthere](https://github.com/thekevinscott/putitoutthere) (`putitoutthere.toml` at the repo root; `release.yml` calls its `@v0` reusable workflow on every push to `main`). Each merge whose changed files match a package's release `globs` publishes that package to its registry immediately, with no version bump, tag, or human sign-off in between — and `depends_on` cascades: a `packages/rust/**` change republishes the PyPI and npm packages too. A merge matching no package's globs (root config, docs, CI) publishes nothing. This is why agents shepherd PRs to green and **stop**: merging is publishing, and only the maintainer merges.

## PR Sizing and Issues

- **Every PR is M or smaller.** A change larger than M is broken into a sequence of smaller PRs, each independently reviewable and mergeable. Size by review surface, not raw line count -- a mechanical rename spanning many files can be M, while a subtle core change of far fewer lines may not be.
- **Stack that sequence; do not serialize it.** When an epic's slices depend on each other, open each PR with its `base` set to the branch below it rather than waiting for that branch to merge. Reviewers see each layer's own diff, and the slices land in order without anyone idling. [Stacked pull requests](https://github.blog/changelog/2026-07-30-stacked-pull-requests-are-now-in-public-preview/) are GitHub-native, but the mechanism is just the base branch, so it works from `mcp__github__create_pull_request` (set `base`) without any preview-only tooling -- relevant because `gh` (and therefore the `gh-stack` skill) is unavailable in the hosted sandbox. Three things are already true and need no special handling: **GitHub retargets a child to `main` automatically** when its parent merges; **branch protection and required checks apply per layer**, unchanged; and **diff-scoped gates measure the layer, not its ancestors** -- every CI workflow passes `base: ${{ github.event.pull_request.base.sha }}`, so `unit mutation`, changed-lines coverage and `changelog-gate` all compute against the stack parent (verified on #703 while stacked: `BASE: 0241d80…`, the tip of `claude/699-read-pdf`). Only slices with a real dependency belong in one stack; independent slices are ordinary PRs off `main`.
- **Never request the maintainer's review on a PR.** Do not add reviewers by any mechanism -- `gh pr create --reviewer`, `gh pr edit --add-reviewer`, the GitHub API, or MCP tools. Each request fires a notification; the workflow is already shepherd-to-green and report status, with the maintainer deciding when to look and merge. Opening the PR is the signal; a review request adds nothing but noise.
- **Every PR is accompanied by a GitHub issue it auto-closes.** Put a closing keyword (`Fixes #<n>` / `Closes #<n>`) in the PR body so the issue closes on merge; file the issue first if one does not exist yet.
- **Every PR gets a unique branch name -- NEVER reuse a branch name across PRs, even for follow-up work on the same issue/epic.** After a PR merges (or when starting a new PR), branch a fresh, distinctly-named branch; do **not** re-create the just-merged branch name and stack the next change on it. Reuse hides real code divergence: an unrelated PR that merged to `main` in the meantime can textually auto-merge with your branch into a **compile error** (e.g. `main` adds a call to a helper your branch deleted). A unique branch per PR forces an honest three-way merge against current `main`. It also keeps each e2e attestation receipt on its own path (`packages/<pkg>/e2e-attestations/<branch>.json`), so a re-used name does not have two PRs writing the same file. (Learned the hard way in epic #601: #602 and #603 both used `claude/tackle-601-kxeykd`.)

## Merge Conflict Resolution

**Merge conflicts on in-flight PRs are the highest priority.** When asked to resolve a merge conflict:

1. Immediately stop other work and focus on the conflict.
2. Pull the latest main/base branch.
3. Resolve the conflict carefully, preserving both intended changes where possible.
4. Commit and push the resolution.
5. Do not proceed with other tasks until the merge conflict is fully resolved and the branch is clean.

## PR Monitoring

Merge gating is via **pr-monitor** (`.github/workflows/pr-monitor.yml`, the **`CI Gate`** check — [thekevinscott/pr-monitor](https://github.com/thekevinscott/pr-monitor) at the rolling `@v1`). It is an *aggregator*, not a test, and it does not guess at the check set: [willfire](https://github.com/thekevinscott/willfire) evaluates the repo's workflow files against the PR's base branch, changed files and head commit to get the exact set of workflow runs GitHub will create, and the gate is a set comparison against the runs on the head — yellow while a predicted run is missing or unfinished, red when one concludes anything but `success` / `skipped` / `neutral` / `stale`, and red when a run appears that nothing predicted. Branch protection requires only `CI Gate`, so adding/renaming/removing jobs or workflows needs no branch-protection or gate-config coordination — don't flag that concern.

**Zero predicted runs is a pass, and that is a derived verdict rather than a blind spot.** Post-#834 a diff can legitimately trigger nothing (docs, `agents/`, `notes/`); the gate reads the same workflow files GitHub does, so an empty required set means the diff really does trigger no checks. Two corollaries: a path-filtered workflow still in flight **does** hold the gate (it was predicted, so it is required — the opposite of the check-counting era), and a workflow that should have dispatched but didn't hangs the gate instead of passing silently. #862 would have closed the residual CI-config-only gap with a `ci-paths` check; it is closed `not_planned` because prediction subsumes most of it.

**There is nothing to tune** — `github-token` is the whole input surface, and `pre-sleep` / `minimum-checks` / `timeout` / `job-name` / `excluded-jobs` were removed upstream when prediction landed (GitHub ignores them with a warning, so a config still passing them is doing nothing). `timeout-minutes: 20` on the job is the backstop, not a gate setting. When the gate is red or stuck, **read its log first — it prints `Required: [...]`, the prediction**: `Non-passing runs` is an ordinary CI failure, an *unexpected* run is a willfire modelling gap, and a timeout is either over-prediction or a hung job (diff `Required:` against the runs that exist to tell which). Full semantics, the configuration constraints, per-shape debugging, and why check-run counting could not work: `agents/reference/pr-monitor.md`.

**Gate changes are testable on the PR that makes them.** The gate is an ordinary workflow, so a PR editing `pr-monitor.yml` is gated by its own edited copy — unlike Mergify, which read config from `main` only and forced land-and-canary cycles to test anything. **Flaky jobs are manual**: re-run by hand to unblock, then fix or quarantine.

**Auto-merge is GitHub-native:** enable auto-merge on the PR (UI or `mcp__github__enable_pr_auto_merge`) and it merges once branch protection is satisfied — `CI Gate` green plus an approving review. The old Mergify `auto-merge` label does nothing now.

When monitoring PRs to get them across the finish line (shepherding to green):

1. **Watch for merge conflicts** in addition to CI status. If a PR becomes unmergeable due to conflicts, immediately flag and work to resolve.
2. **Monitor for GPG signing failures** if the repo requires signed commits. Re-sign or re-commit as needed to pass signature checks.
3. Check CI logs for any signing-related errors and address them before merge.
4. Keep the user informed of blockers and resolution status.

## Manually Exercise New Features

**Before declaring a feature done, run it.** Build the code (`pnpm build`, `uv run maturin develop`, `cargo build`, etc.) and exercise the user-visible behavior at least once -- spawn the CLI, hit the endpoint, import the SDK, send a real request. Capture the observed output and confirm it matches the spec.

Tests are necessary but not sufficient: a passing unit test proves the function does what the test says; a passing integration test proves the public surface works in a contrived harness. Neither catches things like a wrong file path in a docstring, a startup script that errors before any test imports it, a configuration that silently no-ops in CI but fails in production-shape, or a serialization difference that the spec but no test specifies. The manual run closes that gap.

Note the run in the PR body alongside the e2e verification block -- one or two lines is enough (the command, the input, what was observed). Future agents reviewing the PR should be able to reproduce it.

## Cross-SDK Parity (PARITY.md)

`PARITY.md` is a **living document** that tracks API-surface parity across the Python, Rust, and TypeScript SDKs. It must stay current.

On every PR that touches any SDK's public API (`packages/python/dirsql/`, `packages/rust/src/`, or `packages/ts/src/`), the author must:

1. Update `PARITY.md` to reflect the new/changed/removed API surface.
2. Call out in the PR body whether the change is **introducing parity drift** (one SDK gets something the others don't yet) or **restoring parity** (bringing a lagging SDK in line). Drift is allowed but must be intentional and tracked.
3. If drift is introduced, open a follow-up GitHub issue for each lagging SDK so the gap is visible.

Orchestrators must block merges of SDK-touching PRs that don't update `PARITY.md`.

## Benchmarks

Run `cargo bench -p dirsql` after significant changes to the Rust codebase. Not in CI -- local only. Covers: SQLite operations, directory scanning, row diffing, glob matching. Use to catch performance regressions before merging.
