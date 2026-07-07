# docs: README doesn't document `[e2e].extra_scope` / `--extra-scope` (the #333 shared-core freshness feature)

_Paste-ready issue for `thekevinscott/testing-conventions`. dirsql's session GitHub scope is locked to `thekevinscott/dirsql`, so it could not be filed from there directly._

## Summary

The shared-core e2e-freshness feature shipped in **#333** — the `e2e verify --extra-scope` / `--exclude` flags, the `[e2e].extra_scope` / `[e2e].exclude` config keys `detect.py` reads, and the reusable workflow's `e2e_extra_scope` / `e2e_exclude` wiring — is **not documented in the README**. It's the exact feature a native monorepo needs (a shared core bound into several bindings, where a change to the core must stale each binding's e2e attestation), and it's fully implemented, but a consumer can only discover it by reading `--help`, the `detect` action, or the workflow source.

## What's undocumented

Present in the code, absent from the README:

- **CLI flags** (`e2e verify --help`):
  - `--extra-scope <DIR>` — "Extra freshness roots (#333): repo-root-relative directories outside `path` whose commits join the freshness walk — a shared source tree beside the package (a native core bound into several bindings) that no `--scope` at-or-below `path` can reach. Repeatable; the attestation must name the newest in-range commit touching the union of `--scope` and every `--extra-scope`."
  - `--exclude <DIR>` — "Feature-gated subtrees carved back out of the `--extra-scope` union (#333): repo-root-relative directories (a core `cli/` compiled out of the bindings) whose commits must not stale the attestation. Repeatable."
- **Config keys** (`detect` action, read from the discovered config file): `[e2e].extra_scope` and `[e2e].exclude`.
- **Reusable-workflow wiring**: the `e2e-verify` job passes `$EXTRA_SCOPE $EXCLUDE` from `needs.detect.outputs.e2e_extra_scope` / `e2e_exclude`, "empty when the package declares no `[e2e]` roots."

## Why it matters

Without docs, a monorepo consumer concludes (as this one did) that the "shared core is outside every package's `--scope`" case is **unsolvable with current tooling** and either keeps a bespoke freshness script or files an upstream feature request — for a feature that already exists. A short README section closes that gap.

## Suggested addition

An `e2e` config section in the README documenting:

```toml
# A source tree compiled into this package but living outside it (e.g. a native
# core bound into several bindings). Changes here stale this package's e2e
# attestation even though they're outside its --scope.
[e2e]
extra_scope = ["packages/rust/src"]
# Feature-gated subtrees of the extra scope that are NOT compiled into this
# package, so their changes must NOT stale it.
exclude = ["packages/rust/src/cli", "packages/rust/src/bin"]
```

…plus one line each on the matching `e2e verify --extra-scope/--exclude` flags and the reusable-workflow behavior (config-driven, empty by default).

## Environment

Reusable workflow `@v0` (`6ba7a9e`); `e2e verify` `--extra-scope`/`--exclude` and the `[e2e]` config live and correct — this is docs-only. Consumer: dirsql, `packages/{python,ts}` bindings over the `packages/rust` core.
