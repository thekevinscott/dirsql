# Reusable-workflow gates (testing-conventions): adoption & debugging

Extracted from AGENTS.md (see "CI Workflows" there for the summary). This is the full operational reference.


Six per-domain workflows call the `testing-conventions` reusable workflow at the **moving tag `@v0`** (see #240 and its sub-issues). #861 split the single `conventions.yml` into them so GitHub's workflow-scoped `paths:` filters could triage per lane:

| Workflow | Caller jobs |
| --- | --- |
| `dirsql-python-ci.yml` | `python-sdk`, `rust-python-binding` |
| `dirsql-typescript-ci.yml` | `typescript-sdk`, `rust-napi-binding` |
| `dirsql-rust-ci.yml` | `rust` |
| `internals-checks-ci.yml` | `internals-checks` |
| `internals-distcheck-ci.yml` | `internals-distcheck` |
| `plugin-dirsql-embedding-ci.yml` | `plugins-embeddings` |

The eight caller jobs are the pre-split eight lanes, job ids unchanged -- so check-run names (`<job id> / <gate>`) survived the move. Hard-won operational rules:

- **`@v0` rolls; a removed/renamed input startup-fails the WHOLE calling workflow.** When upstream moves an input (e.g. #289 removed `build_command`, migrating it to config), any caller still passing it fails with `startup_failure` and **0 jobs** -- and it takes down that workflow on `main` and every open PR, not just the PR that added it. Every caller passes the same input set, so one removed input reds all six at once; the split changed the blast radius' shape, not its size. An input change is a live breakage; fix it fast.
- **On `startup_failure`, read the run annotation FIRST -- do not hypothesize.** There are no job logs (no jobs ran), so `get_job_logs` is empty; instead WebFetch the run's `html_url`, which surfaces `Invalid workflow file: <workflow>.yml#L<n> -- Invalid input, <name> is not defined in the referenced workflow`. That names the file, line, and cause exactly. Guessing "tag-roll / transient / the gate itself" wastes rerun cycles; the annotation is authoritative.
- **`testing-conventions.toml` is validated strictly -- an unknown key fails EVERY gate.** A bad `[<lang>].<key>` (e.g. `[typescript].build_command`, which does not exist -- `[typescript]`/`[rust]` accept only `coverage`/`exempt`; `build_command` is `[python]`-only) makes the CLI reject the whole config, so every gate (even colocated-test) goes red. Native builds **auto-provision from the manifest** (maturin / napi / `Cargo.toml`): a napi/TS package needs **no** `build_command` at all -- keep `rust_toolchain: true` to supply cargo, nothing else.
- **Read reusable-workflow behavior at a PINNED commit, and trust `MIGRATIONS.md` over probing.** `git ls-remote https://github.com/thekevinscott/testing-conventions v0` for the sha, then WebFetch the raw workflow at that sha (`@v0` can roll mid-run and spuriously `startup_failure` a probe). Before probing a gate, cross-check upstream `packages/<lang>/MIGRATIONS.md` (authoritative): "landed" often means the CLI *capability* shipped while the reusable *job* isn't wired to it yet (e.g. `e2e verify --scope` shipped one release before the job passed it).
- **e2e attestation goes stale after a squash merge.** The merged attestation names the pre-squash commit, which is dangling on `main`; the reusable `e2e-verify` job runs unconditionally, so it reds. `e2e verify --scope <path>` narrows the freshness walk to the source dir so test/docs-only commits don't stale it. A stale-attestation red is **our** re-attest to fix (as the last commit touching that package -- mind the ordering trap: `attest` records `HEAD`, so attest right after the package's own last change), never a tool bug.
- **Never retire a bespoke gate ahead of a green proof.** Adopt a gate by *probing* (add the gate to the caller, keep the bespoke workflow), confirming green per-job, then retiring the bespoke workflow in a follow-up. A red is diagnosed and fixed or filed, never hidden.

#### Adoption state (post-#240, 2026-07-08)

Every adoptable gate runs inside the per-domain CI workflows, per package; `testing-conventions.toml` carries **zero** exemptions. The map:

| Gate | python | typescript | rust (core) |
| --- | --- | --- | --- |
| colocated-test (+ co-change) | ✅ | ✅ | ✅ (presence-only) |
| unit-lint | ✅ | ✅ | ✅ |
| integration-lint | ✅ | ✅ | ✅ |
| unit-coverage | ✅ | ✅ | ❌ bespoke in `dirsql-rust-ci.yml` -- permanent, see below |
| mutation | ✅ | ✅ | ✅ |
| e2e-verify | ✅ | ✅ | N/A by design, see below |
| packaging | ✅ | ✅ | ✅ (workspace-member support, upstream #360/#362) |

The binding crates (`packages/ts/napi`, `packages/python`) additionally run colocated-test + unit-lint at their own crate roots (#405).

- **Rust unit-coverage is bespoke PERMANENTLY, not pending upstream.** The `branch` floor needs nightly (`cargo llvm-cov --branch`); the reusable coverage job provisions stable, and upstream declined a toolchain input. The sanctioned alternative -- a crate-root `packages/rust/rust-toolchain.toml` -- was probed in #437 and **breaks the release build**: the release precheck cross-compiles from `packages/rust` on stable with added musl/darwin targets, and the pin switched that build to nightly (`E0463: can't find crate for core` on every target). One crate, one directory, two callers needing different channels -- a crate-root pin cannot serve both, so `dirsql-rust-ci.yml`'s coverage job scopes nightly to its own step. Revisit only if upstream gains a toolchain selector that does not leak to other builds of the same crate.
- **Rust has no e2e-verify because it has nothing to attest.** Attestations are receipts for suites CI cannot run (they spawn the shipped binary through the real launchers); rust's outermost tier (`packages/rust/tests/`) runs directly in CI on every PR, so a receipt would be strictly weaker evidence than the run itself. Core changes are still e2e-freshness-enforced through BOTH bindings via `[e2e].extra_scope` (see *E2E Attestation*).
