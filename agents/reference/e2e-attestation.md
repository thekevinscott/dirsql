# E2E Attestation — full mechanics

Extracted from AGENTS.md (see "E2E Attestation" there for the summary).


CI does not run the e2e suites -- they need real binaries, and some need live LLM calls -- but it enforces, **per package**, that a branch touching that package's source carries a **receipt** saying they were run. Receipts are **one JSON file per branch** in the package's `e2e-attestations/` directory -- `packages/python/e2e-attestations/<slug>.json`, `packages/ts/e2e-attestations/<slug>.json` -- each recording (via [`testing-conventions`](https://github.com/thekevinscott/testing-conventions)) the e2e command, its exit code, the commit it ran against, and the branch. `internals/checks` and `plugins/dirsql-plugin-embeddings` carry the same directory.

The `<slug>` is the branch name standardized: `/` becomes `-` and the whole thing is lowercased, so `claude/issue-982-xy1u9u` writes `claude-issue-982-xy1u9u.json`. Ask the tool rather than deriving it by hand -- `testing-conventions e2e slug [BRANCH]` prints it (default: the checked-out branch). Attest needs a checked-out branch: a detached HEAD has no slug and the command refuses.

The check runs **inside the reusable workflow** (`dirsql-python-ci.yml` / `dirsql-typescript-ci.yml`, the `python-sdk` / `typescript-sdk` `e2e-verify` gate): `e2e verify "$PACKAGE_ROOT" --scope "$SCAN_PATH" --base "$BASE"` reads the **`base...HEAD` branch diff** -- a branch whose diff leaves the scoped source untouched owes no receipt, and one that changed it passes once that same diff adds or updates a receipt under `<package>/e2e-attestations/`. It is a **receipt gate, not a test runner** -- no suite, no build, and no LLM run in CI, so it does not violate the E2E Test Policy above. The bespoke `e2e-attestation.yml` this replaced is deleted (whole-hog adoption -- the reusable `e2e-verify` gate owns per-package receipts).

The `--scope` + `--base` diff-scoping makes the gate per-SDK by construction: a change under `packages/python/dirsql` demands a receipt only from the python lane, a change under `packages/ts/src` only from the ts one, and a PR that does not touch a package's scoped source never verifies it. Each package's `[e2e].extra_scope` widens *its own* scope by the code compiled into it (below) without breaking that property.

**Write a receipt for each package you changed** before you push. From the repo root:

```bash
just e2e-attest-python   # cd packages/python && testing-conventions e2e attest 'just test-e2e'
just e2e-attest-ts       # cd packages/ts && testing-conventions e2e attest 'pnpm test:e2e'
```

`attest` runs the command, writes `<package>/e2e-attestations/<slug>.json` for the checked-out branch, and commits it for you. It exits with the command's own code and only records a receipt when the command passes -- the command is the judgment being recorded, so a full suite, a targeted subset, or a deliberate no-op are all valid things to attest.

**Order does not matter.** The gate reads the branch diff, not commit ancestry, so a later source commit under the package does not invalidate an earlier receipt, and a multi-package PR can attest both packages at the end. That also makes it **indifferent to rebases and squash merges** -- the receipt's `commit` field is recorded for the record, not compared against `HEAD`. The retired "the attestation must be the last commit touching that package" rule described the single-file `e2e-attestation.json` layout that per-branch receipts replaced; it no longer holds.

**One receipt path per branch is why branch names are never reused** (AGENTS.md, "PR Sizing and Issues"): two PRs sharing a branch name write the same `<slug>.json` and collide.

Receipts are append-only in practice -- a merged branch's receipt stays in the directory as the record that its suite ran. They are not source: the `changelog-gate` exempts `e2e-attestations/`, `release-ci.yml` excludes it from the publish globs, and `putitoutthere.toml` keeps it out of every shipped artifact.

**Everything compiled into the binding is in scope -- CI-enforced via `[e2e].extra_scope`.** The shared Rust core (`packages/rust/src`) is compiled into both bindings but lives in neither binding's subtree, and `e2e verify --scope` requires the scope to be a **descendant of the caller's `path`** -- so a binding caller cannot point its own `--scope` at the core. That case is solved by the `e2e verify --extra-scope` / `--exclude` flags (upstream #333): an `[e2e]` block declares the extra roots, `detect.py` reads it, and the reusable `e2e-verify` job passes it through. Pure config; the former bespoke `e2e_core_freshness.py` gate is not needed. Two roots per binding:

- **The core, `packages/rust/src` -- puts BOTH bindings in scope** (#337). **After any shared-core change, attest each binding** (`just e2e-attest-python`, `just e2e-attest-ts`) -- CI enforces this, it is no longer a by-hand-only promise. **`cli`-only** core source (`packages/rust/src/cli/**`, `packages/rust/src/bin/**`) is feature-gated out of the binding *libraries*, but since #721 each launcher calls the core's `run_cli` **in-process through its binding** -- a CLI change alters what those suites exercise, so it demands a receipt from both bindings like any other core change (no `exclude` carve-out).
- **The binding crate itself -- puts ONLY its own package in scope** (#933): `packages/python/src` (the PyO3 glue) for python, `packages/ts/napi` (the napi-rs crate, manifest and `build.rs` included) for typescript. Same #721 argument one layer out: `run_cli` in each binding does argv framing, GIL detach / exit-code handling, and runs in every e2e case, so a binding-only diff changes what the suites exercise with no core change at all. A napi change has no bearing on the python suite and never demands a python receipt, and vice versa.

**That per-package split is why `packages/python` and `packages/ts` each carry their own `testing-conventions.toml`** (passed explicitly as the lane's `config:`, the pattern `internals/checks` established in #550). `[e2e]` is a single global table applied to every caller reading a given config, so "core + *my* binding" is not expressible in one shared file: declaring both bindings at the repo root would make a napi change demand a python receipt (and force `packages/ts/napi/**` into the python lane's path triggers to enforce it at all). The root `[e2e]` block stays as the default for a caller with no config of its own.

Proven over synthetic single-root diffs (`e2e verify packages/<pkg> --scope <src> --base main`): a `packages/ts/napi`-only diff is exit 0 with the core root alone and exit 1 once `--extra-scope packages/ts/napi` is added, while the python lane's flags stay exit 0 over that same diff; the mirror holds for `packages/python/src`.

CI installs the latest `testing-conventions` release (unpinned); install it locally before attesting: `pip install testing-conventions`. In the hosted sandbox that build fails -- use `uvx testing-conventions e2e attest '<cmd>'` instead (`agents/build/environment.md`).
