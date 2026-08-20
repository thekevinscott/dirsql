# Merge gating (pr-monitor): semantics & debugging

Extracted from AGENTS.md (see "PR Monitoring" there for the summary). This is the full operational reference.

The gate is `.github/workflows/pr-monitor.yml`, whose single job is named **`CI Gate`** — the only context branch protection requires. It runs [thekevinscott/pr-monitor](https://github.com/thekevinscott/pr-monitor) at `@v1`, a rolling tag tracking that repo's `main`.

## How the verdict is computed

The action does not guess at the check set. [willfire](https://github.com/thekevinscott/willfire) evaluates the repo's workflow files against the PR's base branch, changed files and head commit, returning the entries GitHub will actually create; everything willfire does not mark `no-dispatch` becomes a **required workflow file**. The gate then polls `listWorkflowRunsForRepo` for the head commit every 5s, keeps the `pull_request` runs, and compares sets:

| Observation | Verdict |
| --- | --- |
| a required run is missing, or exists but unfinished | keep waiting (yellow) |
| every required run finished `success` / `skipped` / `neutral` / `stale` | pass |
| a required run finished anything else (`failure`, `cancelled`, `timed_out`, `action_required`, `startup_failure`) | fail |
| a run exists that nothing predicted | fail immediately |

Both sides exclude the gate's own workflow *file*, read from `GITHUB_WORKFLOW_REF`.

**Why workflow runs, not check runs.** willfire predicts at job granularity, but the comparison happens at run granularity: a run stays non-terminal until *all* of its jobs finish — `needs:`-gated jobs and reusable-workflow (`workflow_call`) children included — so "the run finished" already means "every job finished", with no transient gap to race against. It also makes willfire's job-level `unknown` verdicts harmless: a matrix computed at runtime from another job's output cannot be expanded statically, but the run exists either way and still has to go green.

## Configuration

`github-token` is the entire input surface. `pre-sleep`, `minimum-checks`, `timeout`, `check-interval`, `job-name` and `excluded-jobs` were all **removed upstream** when prediction landed — each existed only to compensate for not knowing the expected run set. GitHub warns and ignores inputs a composite action does not declare, so ours sat inert in `pr-monitor.yml` until they were dropped; a config still passing them is silently doing nothing.

Three structural requirements:

- **`permissions:` needs all three** of `actions: read`, `contents: read`, `pull-requests: read`. The last one is willfire reading the PR and its changed files; it postdates the check-count era, so an older block is missing it.
- **The gate stays alone in its workflow.** The action excludes its own workflow file from both sides of the comparison, so a sibling job added to `pr-monitor.yml` would go entirely unmonitored.
- **`timeout-minutes` is the backstop.** The action has no timeout of its own (see its `monitor.ts`) and would poll forever; the job-level 20 kills it instead. Post-#834 fan-out completes in ~3 minutes, so 20 is generous. The `concurrency` group with `cancel-in-progress` covers the other half: a gate run resolves its target SHA at startup, so once a new commit lands the old run is polling a SHA nobody cares about — cancelling it frees the runner rather than letting it finish.

## Debugging a red or stuck gate

Every run logs `Required: [...]` — the prediction. Read it before hypothesizing.

- **`Non-passing runs: [...]`** — an ordinary CI failure, in the workflow named. Fix that.
- **An unexpected run** — a run exists that willfire did not predict, so the gate fails rather than vouch for a set it cannot explain. This is a *modelling* gap: willfire does not model `workflow_run` chains or `pull_request_target`, and it infers `opened` vs `synchronize` from the PR's commit count rather than the real event ([willfire#2](https://github.com/thekevinscott/willfire/issues/2)), so a workflow narrowing `types:` can have its verdict flipped by a wrong guess. Every dirsql workflow is on a bare `on: pull_request` (with `paths:` / `branches:`, never `types:`), so this firing means a new workflow shape crept in.
- **A timeout** — over-prediction or a hang. Diff the logged `Required:` list against the runs that exist. A required run that never dispatched is over-prediction: the workflow's filters and willfire disagree, and the workflow is the thing to look at. A run that dispatched and sat is a hung job — open it and look at the stuck job's current *step*: a slow job advances, a hung one sits in one step while its siblings finish in seconds. Cancel it rather than letting it burn the timeout. Learned on #727, where a test awaited an event that a behavior change meant would never arrive: three jobs sat in one step for 35+ minutes and it was read as flake twice before anyone looked at the steps. Because the gate treats `cancelled` as a failure, a cancelled hung job needs a re-run of that workflow (and of the gate) once fixed.

**The required-check context is the job's `name:`**, not the workflow's. Renaming `CI Gate` leaves every open PR reporting the old name until it gets a fresh run — merge the base branch in, or push, to retrigger. GitHub does not re-evaluate an existing run against the new requirement.

**Flaky jobs are manual.** Mergify's CI Insights auto-retry left with Mergify. Re-run a flaky job by hand to unblock, then fix or quarantine it; the gate polls live state, so a re-run that goes green flips the gate without re-triggering anything else. The gate's own run can be re-run too, if it timed out against a since-fixed hang.

## Why not check-run counting (the Mergify era, #830–#947)

A gate built on check-run counts cannot express "everything expected has finished".

- `#check-failure = 0` alone is vacuously true before CI dispatches and while it runs — a gate that does not gate (#943 measured it green 2m21s before the last check finished).
- Adding `#check-pending = 0` deadlocks: Mergify counts its own in-progress check, so the count never reaches zero (#832; confirmed deliberately by canary #946, whose check summary showed an empty waiting-checks list with the pending condition unsatisfied).
- Named per-workflow anchors were the remaining option, and path filtering (#834) killed them: an anchor on a workflow that does not run pends forever.

Predicting the run set sidesteps all three — the expected set is computed per PR instead of enumerated in config — at the cost of one runner for the CI duration. It also restores a property Mergify never had: **gate changes are testable on the PR that makes them**, since the gate is an ordinary workflow and a PR editing `pr-monitor.yml` is gated by its own edited copy. Mergify read its config from `main` only, forcing land-and-canary cycles to test anything.

## The zero-check policy

**A PR whose prediction is empty passes.** Post-#834 path filtering a diff can legitimately trigger nothing (docs, `agents/`, `notes/`, root `*.md`); a `[skip ci]` commit predicts nothing either.

Under check-counting this was an accepted blind spot — the gate could not tell "nothing should run" from "nothing has started yet". Under prediction it is a derived verdict: the gate reads the same workflow files GitHub does, so an empty required set means the diff really does trigger no checks. Two consequences follow.

1. **A path-filtered workflow still in flight now holds the gate.** It was predicted, so it is required, and the gate stays yellow until its run finishes. The old corollary — that an in-flight filtered workflow did not hold the gate — is obsolete.
2. **A workflow that should have dispatched but didn't now hangs the gate** rather than passing silently, surfacing as an over-prediction timeout.

#862 was to close the residual CI-config-only gap with a `dirsql-checks ci-paths` check asserting that workflow filters and the gate's config agree. It is closed `not_planned`: prediction subsumes most of it, since a config-only PR gets its expected set computed from the very files it edits.
