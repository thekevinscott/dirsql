# E2E Attestation — full mechanics

Extracted from AGENTS.md (see "E2E Attestation" there for the summary).


CI does not run the e2e suites -- they need real binaries, and some need live LLM calls -- but it enforces, **per package**, that they *were* run against that package's current code. Each SDK package carries its own attestation at its root -- `packages/python/e2e-attestation.json` and `packages/ts/e2e-attestation.json` -- recording (via [`testing-conventions`](https://github.com/thekevinscott/testing-conventions)) the e2e command, its exit code, and the commit it ran against. The freshness check runs **inside the reusable workflow** (`dirsql-python-ci.yml` / `dirsql-typescript-ci.yml`, the `python-sdk` / `typescript-sdk` `e2e-verify` gate): `e2e verify "$PACKAGE_ROOT" --scope "$SCAN_PATH" --base "$BASE"` measures freshness over the **`base..HEAD` scoped-source diff**, so a PR that does not touch the SDK source has nothing to verify. It is a **freshness gate, not a test runner** -- no suite, no build, and no LLM run in CI, so it does not violate the E2E Test Policy above. The bespoke `e2e-attestation.yml` this replaced is deleted (whole-hog adoption -- the reusable `e2e-verify` gate owns per-package freshness).

The `--scope` + `--base` diff-scoping makes the gate per-SDK by construction: a change under `packages/python/dirsql` stales only the python attestation, a change under `packages/ts/src` only the ts one, and a PR that does not touch a package's scoped source never verifies it. Each package's `[e2e].extra_scope` widens *its own* scope by the code compiled into it (below) without breaking that property.

**Regenerate the attestation for each package you changed**, as the last commit touching that package before you push. From the repo root:

```bash
just e2e-attest-python   # cd packages/python && testing-conventions e2e attest 'just test-e2e'
just e2e-attest-ts       # cd packages/ts && testing-conventions e2e attest 'pnpm test:e2e'
```

`attest` runs the command, writes `<package>/e2e-attestation.json` naming the current commit, and commits it for you. **The attestation must be the last commit touching that package** -- any later non-attestation commit under the package re-stales it and the gate goes red.

**Multi-package PRs:** because `attest` records `HEAD`, attest each package right after finishing *its* changes (complete + attest python, then complete + attest ts). Attesting both only at the very end leaves whichever you attest second naming the other's attestation commit -- outside its subtree -- which verify rejects.

**Everything compiled into the binding is in scope -- CI-enforced via `[e2e].extra_scope`.** The shared Rust core (`packages/rust/src`) is compiled into both bindings but lives in neither binding's subtree, and `e2e verify --scope` requires the scope to be a **descendant of the caller's `path`** -- so a binding caller cannot point its freshness `--scope` at the core. That case is solved by the `e2e verify --extra-scope` / `--exclude` flags (upstream #333): an `[e2e]` block declares the extra roots, `detect.py` reads it, and the reusable `e2e-verify` job passes it through. Pure config; the former bespoke `e2e_core_freshness.py` gate is not needed. Two roots per binding:

- **The core, `packages/rust/src` -- stales BOTH bindings** (#337). **After any shared-core change, re-attest each binding** (`just e2e-attest-python`, `just e2e-attest-ts`) -- CI enforces this, it is no longer a by-hand-only promise. **`cli`-only** core source (`packages/rust/src/cli/**`, `packages/rust/src/bin/**`) is feature-gated out of the binding *libraries*, but since #721 each launcher calls the core's `run_cli` **in-process through its binding** -- a CLI change alters what those suites exercise, so it stales the attestations like any other core change (no `exclude` carve-out).
- **The binding crate itself -- stales ONLY its own package** (#933): `packages/python/src` (the PyO3 glue) for python, `packages/ts/napi` (the napi-rs crate, manifest and `build.rs` included) for typescript. Same #721 argument one layer out: `run_cli` in each binding does argv framing, GIL detach / exit-code handling, and runs in every e2e case, so a binding-only diff changes what the suites exercise with no core change at all. A napi change has no bearing on the python suite and does not stale it, and vice versa.

**That per-package split is why `packages/python` and `packages/ts` each carry their own `testing-conventions.toml`** (passed explicitly as the lane's `config:`, the pattern `internals/checks` established in #550). `[e2e]` is a single global table applied to every caller reading a given config, so "core + *my* binding" is not expressible in one shared file: declaring both bindings at the repo root would make a napi change demand a python re-attest (and force `packages/ts/napi/**` into the python lane's path triggers to enforce it at all). The root `[e2e]` block stays as the default for a caller with no config of its own.

Proven over synthetic single-root diffs (`e2e verify packages/<pkg> --scope <src> --base main`): a `packages/ts/napi`-only diff is exit 0 with the core root alone and exit 1 once `--extra-scope packages/ts/napi` is added, while the python lane's flags stay exit 0 over that same diff; the mirror holds for `packages/python/src`.

CI installs the latest `testing-conventions` release (unpinned); install it locally before attesting: `pip install testing-conventions`.
