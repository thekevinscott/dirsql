# e2e-verify: detect shared-core staleness for monorepo bindings (`--scope` can't reach a sibling source tree)

_Paste-ready issue for `thekevinscott/testing-conventions`. dirsql's session GitHub scope is locked to `thekevinscott/dirsql`, so it could not be filed from there directly._

## Summary

`e2e verify` can prove a package's e2e attestation is fresh **with respect to its own subtree**, but in a monorepo where several packages are compiled from a **shared source tree that lives outside every package's subtree**, it cannot see that a change to that shared tree has staled a package's attestation. This is the last gap keeping a consumer (dirsql) from deleting its bespoke e2e-freshness tooling entirely.

## Concrete shape (dirsql)

- `packages/rust/src/**` is the **shared core**. It is compiled into **both** the Python binding (`packages/python`) and the TypeScript binding (`packages/ts`) via PyO3 / napi.
- It lives in **neither** binding's subtree.

The reusable `e2e-verify` job runs, per binding:

```
e2e verify "$PACKAGE_ROOT" --scope "$SCAN_PATH" --base "$BASE"
```

where `--scope` must be a **descendant of the caller's `path`** (e.g. `packages/python/dirsql`). So a binding caller **structurally cannot** point its freshness walk at `packages/rust/src` — it's outside `path`. Result: a shared-core change that changes CLI/binding behavior does **not** stale either binding's attestation, so `e2e-verify` passes while the attestation is genuinely stale.

## Why this isn't covered by the existing `--base` fix

The `--base` diff-scoping (squash-fix) correctly narrows freshness to `base..HEAD` **within `--scope`**. That's exactly the problem: the shared core is *outside* `--scope`, so a core-only PR has an empty in-scope diff and the gate no-ops — which is wrong for a binding whose behavior depends on that core.

## Current bespoke workaround (what we want to delete)

dirsql carried `.github/scripts/e2e_core_freshness.py`: on a **non-`cli`** change under `packages/rust/src/**`, it failed unless that core commit was an ancestor of (or equal to) each binding's attested commit — i.e. each binding must have re-attested after the core change. `cli`-only core (`packages/rust/src/cli/**`, `src/bin/**`) is feature-gated out of both bindings and excluded from the staling set.

(dirsql has now deleted this script and `e2e-attestation.yml` as part of whole-hog adoption, accepting an interim gap: until this lands, a non-`cli` shared-core change is not freshness-gated against the binding attestations. The binding CI tier still runs against the real core, so this is a freshness-promise gap, not a correctness one.)

## Proposed fix

Let a caller declare **extra source roots outside `path`** that feed a package's e2e artifact, and include them in the freshness walk. Sketch:

- A new input / config key — e.g. `extra_scope: '["packages/rust/src"]'` (or `[<lang>].e2e_extra_scope`) — appended to the `base..HEAD` diff set that `verify` checks, with the same "attested commit must be ≥ the latest touching commit" rule.
- Optionally an exclude sub-pattern so feature-gated dirs (`packages/rust/src/cli/**`) don't count, matching the `e2e_core_freshness.py` carve-out.

Alternative: a monorepo-level "component → attestations it feeds" map the tool resolves itself. The input approach is the smaller change and mirrors how `--base`/`--scope` already work.

## Acceptance

- A binding caller can declare its shared-core source dir; a PR that changes that dir (non-excluded) fails the binding's `e2e-verify` until the binding re-attests.
- A `cli`-only / excluded change under the shared tree does not stale the binding.
- dirsql restores full-coverage e2e freshness **entirely** inside `conventions.yml` (no bespoke script).

## Environment

Reusable workflow `@v0`. `e2e verify --scope` (#294) + `--base` diff-scoping both live and correct; this is the remaining monorepo-shared-source case they don't cover. Consumer: dirsql `main`, `packages/{python,ts}` bindings over the `packages/rust` core.
