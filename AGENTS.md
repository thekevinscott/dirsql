# dirsql Development

In your responses, strive for brevity. As concise as possible.

## Architecture

All architectural decisions and constraints (including cross-language parity rules, the one-implementation principle, and SDK design) are in `ARCHITECTURE.md`. Do NOT put architectural information in this file -- AGENTS.md is for workflow and process only.

@agents/build/environment.md

## Scratch Files

Write scratch/temporary files to `/tmp` instead of asking permission. Use unique filenames to avoid collisions with other sessions.
Temporary scripts, including Node or shell helpers, must also be written to `/tmp` and executed from there.

## Session Handoff Doc

Maintain one ongoing handoff doc per session and deliver it to the user as a downloadable markdown file at every **stopping point**: after each major unit of work lands (a push, a green CI run, a finished investigation, a merged PR) or when blocked on user input. A stopping point marks a checkpoint, not the end -- send the doc, then keep working.

- Keep it in the session scratchpad / `/tmp` (e.g. `<scratchpad>/handoff.md`). It is conversation-scoped: NEVER commit it, stage it, or place it anywhere in the repo tree.
- Update the same doc in place and re-send it at each checkpoint (in hosted sessions, attach it via the file-delivery tool; locally, print its path), so the freshest copy sits near the bottom of the conversation.
- Write it standalone, so a brand-new session with zero context can resume from it alone: task + status (done / in progress / next), branches/PRs/issues with numbers and CI state, key decisions and discovered constraints (one-line reasons), exact next commands to run, anything waiting on the user.

Purpose: the prompt cache survives at most an hour of inactivity, so resuming a long conversation after hours away reprocesses the entire history at full cost. A current handoff doc near the end of the transcript lets the user scroll up, grab it, and start a cheap fresh session from the doc instead of resuming the stale one.

## Shell Commands

**Do not chain commands** with `;`, `&&`, or `||`. Chained commands break the per-command permission model -- each command must be evaluated separately, and chaining forces a single bulk approval (or prompt) for the whole pipeline. Run each command as its own call.

Exceptions: piping (`|`) is fine when it's genuinely one logical operation (e.g., `cmd | jq`). Heredocs (`cat <<EOF`) are fine. `cd path && cmd` is NOT fine -- use `cd` as a separate call (or pass absolute paths).

## Comments

Default to no comments. Only add one when the WHY is non-obvious -- a hidden constraint, an invariant, a workaround, something that would surprise a reader. Never write archaeology: no issue/PR references, no "added for the X flow" / "used by Y", no restating what adjacent code already says, no reviewer-directed justification. That belongs in the commit message and PR description, not the file -- it rots as the codebase evolves and the file is never re-read once merged. See #445 (trimmed exactly this style repo-wide) and CHANGELOG.md's entry for it.

## CI Workflows

**Every CI check emits actionable fix instructions on failure.** A failing check must tell the contributor exactly what to change -- the file, command, or trailer to add or edit -- not merely which rule was violated. When a check can detect a *near-miss* (a fix was attempted but malformed), it names the specific defect and how to correct it rather than falling through to a generic "not satisfied" message (e.g. the `changelog-gate` names a fragment file whose name breaks the `YYYY-MM-DD-<slug>.md` convention -- pointing at the exact file -- and its "no fragment" error prints the exact path to add; dirsql#566).

**CI logic lives in scripts, not workflow YAML.** `run:` / `github-script` steps stay trivial glue -- check out, set up a toolchain, invoke one command. Anything with iteration, `case` dispatch, conditionals, or text-munging moves to a check in the `internals/checks` uv package (a click group, one subcommand per check -- see `internals/checks/src/checks/`), invoked as a one-liner (`uv run --project internals/checks dirsql-checks <check>`), and carries **colocated unit tests** (the same testing-conventions standard as the rest of the tree -- `foo.py` ↔ `foo_test.py`). Those tests run under `conventions.yml`'s `internals-checks` job (`unit-coverage` enforces a 100% floor; see "Enforcing Colocation" below for the full gate list). Inline workflow logic is untestable, un-runnable locally, and silently duplicated across runners; a script is none of those.

### Reusable-workflow gates (testing-conventions): adoption & debugging

`conventions.yml` calls the `testing-conventions` reusable workflow at the **moving tag `@v0`** (see #240 and its sub-issues). Hard-won operational rules:

- **`@v0` rolls; a removed/renamed input startup-fails the WHOLE workflow.** When upstream moves an input (e.g. #289 removed `build_command`, migrating it to config), any caller still passing it fails with `startup_failure` and **0 jobs** -- and it takes down `conventions.yml` on `main` and every open PR, not just the PR that added it. So an input change is a live breakage; fix it fast.
- **On `startup_failure`, read the run annotation FIRST -- do not hypothesize.** There are no job logs (no jobs ran), so `get_job_logs` is empty; instead WebFetch the run's `html_url`, which surfaces `Invalid workflow file: conventions.yml#L<n> -- Invalid input, <name> is not defined in the referenced workflow`. That names the file, line, and cause exactly. Guessing "tag-roll / transient / the gate itself" wastes rerun cycles; the annotation is authoritative.
- **`testing-conventions.toml` is validated strictly -- an unknown key fails EVERY gate.** A bad `[<lang>].<key>` (e.g. `[typescript].build_command`, which does not exist -- `[typescript]`/`[rust]` accept only `coverage`/`exempt`; `build_command` is `[python]`-only) makes the CLI reject the whole config, so every gate (even colocated-test) goes red. Native builds **auto-provision from the manifest** (maturin / napi / `Cargo.toml`): a napi/TS package needs **no** `build_command` at all -- keep `rust_toolchain: true` to supply cargo, nothing else.
- **Read reusable-workflow behavior at a PINNED commit, and trust `MIGRATIONS.md` over probing.** `git ls-remote https://github.com/thekevinscott/testing-conventions v0` for the sha, then WebFetch the raw workflow at that sha (`@v0` can roll mid-run and spuriously `startup_failure` a probe). Before probing a gate, cross-check upstream `packages/<lang>/MIGRATIONS.md` (authoritative): "landed" often means the CLI *capability* shipped while the reusable *job* isn't wired to it yet (e.g. `e2e verify --scope` shipped one release before the job passed it).
- **e2e attestation goes stale after a squash merge.** The merged attestation names the pre-squash commit, which is dangling on `main`; the reusable `e2e-verify` job runs unconditionally, so it reds. `e2e verify --scope <path>` narrows the freshness walk to the source dir so test/docs-only commits don't stale it. A stale-attestation red is **our** re-attest to fix (as the last commit touching that package -- mind the ordering trap: `attest` records `HEAD`, so attest right after the package's own last change), never a tool bug.
- **Never retire a bespoke gate ahead of a green proof.** Adopt a gate by *probing* (add the gate to the caller, keep the bespoke workflow), confirming green per-job, then retiring the bespoke workflow in a follow-up. A red is diagnosed and fixed or filed, never hidden.

#### Adoption state (post-#240, 2026-07-08)

Every adoptable gate runs inside `conventions.yml`, per package; `testing-conventions.toml` carries **zero** exemptions. The map:

| Gate | python | typescript | rust (core) |
| --- | --- | --- | --- |
| colocated-test (+ co-change) | ✅ | ✅ | ✅ (presence-only) |
| unit-lint | ✅ | ✅ | ✅ |
| integration-lint | ✅ | ✅ | ✅ |
| unit-coverage | ✅ | ✅ | ❌ bespoke in `rust-test.yml` -- permanent, see below |
| mutation | ✅ | ✅ | ✅ |
| e2e-verify | ✅ | ✅ | N/A by design, see below |
| packaging | ✅ | ✅ | ✅ (workspace-member support, upstream #360/#362) |

The binding crates (`packages/ts/napi`, `packages/python`) additionally run colocated-test + unit-lint at their own crate roots (#405).

- **Rust unit-coverage is bespoke PERMANENTLY, not pending upstream.** The `branch` floor needs nightly (`cargo llvm-cov --branch`); the reusable coverage job provisions stable, and upstream declined a toolchain input. The sanctioned alternative -- a crate-root `packages/rust/rust-toolchain.toml` -- was probed in #437 and **breaks the release build**: the release precheck cross-compiles from `packages/rust` on stable with added musl/darwin targets, and the pin switched that build to nightly (`E0463: can't find crate for core` on every target). One crate, one directory, two callers needing different channels -- a crate-root pin cannot serve both, so `rust-test.yml`'s coverage job scopes nightly to its own step. Revisit only if upstream gains a toolchain selector that does not leak to other builds of the same crate.
- **Rust has no e2e-verify because it has nothing to attest.** Attestations are receipts for suites CI cannot run (they spawn the shipped binary through the real launchers); rust's outermost tier (`packages/rust/tests/`) runs directly in CI on every PR, so a receipt would be strictly weaker evidence than the run itself. Core changes are still e2e-freshness-enforced through BOTH bindings via `[e2e].extra_scope` (see *E2E Attestation*).

## Imports

**Prefer relative imports for intra-package references.** Inside a package (Python or TypeScript), use `from .sibling import x` / `import { x } from "./sibling.js"` rather than the absolute `from packagename.sub.sibling import x` / `from "packagename/sub/sibling"`. Relative paths survive renames, signal that the import is internal, and keep cross-cutting refactors (e.g. the `_cli/` → `cli/` rename in #210) from rippling through every import statement. Absolute imports are appropriate when crossing a package boundary or referring to a public re-export.

## File Naming

**TypeScript filenames are dash-case (kebab-case).** Every `.ts` / `.mjs` / `.cjs` / `.json` file under `packages/ts/` uses kebab-case (`load-native-core.ts`, `resolve-binary.test.ts`, `dirsql.config-raises.mjs`); a single lowercase word (`index.ts`, `die.ts`, `main.ts`) is already valid kebab-case and stays. Only filenames follow this rule -- symbols *inside* a file keep their idiomatic `camelCase` / `PascalCase` names (the function in `resolve-binary.ts` is still `resolveBinary`). The convention is enforced for `src/` and `tests/` by biome's `style/useFilenamingConvention` rule (`filenameCases: ["kebab-case"]`) and applies package-wide (`tools/`, fixtures) by hand. Python (`snake_case.py`) and Rust (`snake_case.rs`) keep their own ecosystem conventions.

**Python test files use the `_test.py` suffix, not the `test_` prefix** -- a test for `foo.py` is `foo_test.py` (colocated unit tests) or `<feature>_test.py` (integration/binding/e2e tests under `tests/`), never `test_foo.py`.

## Manually Exercise New Features

**Before declaring a feature done, run it.** Build the code (`pnpm build`, `uv run maturin develop`, `cargo build`, etc.) and exercise the user-visible behavior at least once -- spawn the CLI, hit the endpoint, import the SDK, send a real request. Capture the observed output and confirm it matches the spec.

Tests are necessary but not sufficient: a passing unit test proves the function does what the test says; a passing integration test proves the public surface works in a contrived harness. Neither catches things like a wrong file path in a docstring, a startup script that errors before any test imports it, a configuration that silently no-ops in CI but fails in production-shape, or a serialization difference that the spec but no test specifies. The manual run closes that gap.

Note the run in the PR body alongside the e2e verification block -- one or two lines is enough (the command, the input, what was observed). Future agents reviewing the PR should be able to reproduce it.

## Testing

### Red/Green Development

Follow **red/green** (test-first) methodology:

1. **Write red integration AND e2e tests first** -- it must capture the desired behavior
2. **Run it and confirm it fails (RED)** -- do NOT proceed until the test turns red reliably. A test that passes before implementation proves nothing.
3. **Push the failing test as its own commit and confirm CI goes red for the right reason** -- the failing test must be committed and pushed on its own, and the CI run for that commit must be observed failing before any implementation is written. Local RED is not enough; CI RED is the gate. The failure must be *relevant*: CI must fail specifically because the new test's assertions are unmet, not because of an unrelated flake, a compile error elsewhere, a pre-existing failure, or an infrastructure hiccup. Open the failing job, confirm the new test is the thing that failed, and confirm the failure message matches the behavior the test asserts. A green run, a skipped run, or a red run that fails for any other reason all mean the test is not proving what it must -- fix the test and re-confirm, do not proceed.
4. **Make the minimal change to pass (GREEN)** -- only then write the implementation, committed and pushed separately so CI flips from red to green.
5. Refactor if needed, keeping tests green

### TDD Order: Outside-In

Tests are written **before** implementation, starting from the outermost layer:

1. **Integration test first** -- proves the feature works from the consumer's perspective
2. **Unit tests** -- written as you implement each module

A feature is not done until integration tests pass and cover the new functionality.

### When to Write What

**Does the commit change the public-facing API?**
- Yes -> **integration test required**, plus unit tests as you go
- No -> Check if adequate integration coverage already exists:
  - Adequate -> unit tests only
  - Gaps -> add the missing integration tests, plus unit tests

**Always write unit tests.** The question is whether you also need integration tests.

### Test Locations

- **Unit tests**: Colocated with source
  - Python: `foo.py` -> `foo_test.py` in same directory
  - TypeScript: `foo.ts` -> `foo.test.ts` in same directory
  - Rust: inline `#[cfg(test)]` module at bottom of each source file
- **Integration tests**: `tests/integration/` -- exercise the **SDK** public API (`DirSQL`, `Table`, `RowEvent`, etc.) **only, never the CLI**. Two subdirectories, run as two CI jobs:
  - `tests/integration/hermetic/` -- **every** third-party dependency mocked (the `notify` watcher, network, future LLM clients, and **SQLite and the filesystem** too -- hermetic since #289: Python patches the `_RustDirSQL` core boundary via `unittest.mock`, TypeScript `vi.mock`s `src/core.ts`). Needs no native build.
  - `tests/integration/binding/` (#289) -- the SDK public API against the **real core** (PyO3 / napi binding, real SQLite, real temp-dir filesystem). Proves the SDK↔core marshaling and real query/watch/persist behavior the hermetic subdir mocks out -- coverage the CLI e2e suites cannot provide, since the CLI is a pure Rust binary that never crosses a binding. Its CI job builds the native artifact (maturin / napi + cargo). This is the only real-core coverage, so unlike e2e it **runs on every PR** -- upstream's integration definition (first-party code runs for real; mocking the outside world is *permitted*, not required) fits it as-is.
  Both **run in CI**. Rust has no binding subdir: it *is* the core, so `packages/rust/tests/` remains its integration tier.
- **E2E tests**: `tests/e2e/` -- exercise the **CLI** only (the `dirsql` binary, the `dirsql interpret` subprocess, the launcher) with **nothing mocked**. **No mocks, no fakes, no monkeypatching. NOT run in CI** -- CI verifies only the per-package *attestation* that they ran (see *E2E Attestation*).
- **Distcheck tests**: **not** an SDK-package tier -- the *functional* publishability flows (build, pack, install, and run the published artifact) live in the `internals/distcheck` package (#520), which itself follows the three-tier layout. **Run in CI** via that package's `dirsql-distcheck python` / `dirsql-distcheck node` entry points (python-test.yml / ts-test.yml `distcheck` jobs). Distinct from the **`packaging` gate** (testing-conventions, run via `conventions.yml` for all three languages), which only asserts no test files *ship* in the built artifact and never installs or runs it.

### Enforcing Colocation (testing-conventions)

The Python/TypeScript/Rust colocation rule above is enforced as a **blocking CI gate** by [`testing-conventions`](https://github.com/thekevinscott/testing-conventions), a config-driven CLI that scans each SDK's source tree and fails on any source file lacking a colocated unit test (for Rust, an inline `#[cfg(test)]` module). The wiring lives in `.github/workflows/testing-conventions.yml` (it pins the CLI version and runs the per-language `unit colocated-test` presence scans, plus a PR-only `--base` co-change check for Python/TypeScript that fails when a modified source's colocated test did not change alongside it) and `testing-conventions.toml` (the exempt list).

The same workflow also runs `unit lint` -- the **isolation** rule: a unit test must mock every collaborator (it must not import an un-mocked one), so the test exercises only the unit under test. It is wired for **all three SDKs** (#233 / epic #231): Python and TypeScript first, then Rust once its effectful-std unit tests were either moved to the integration tier (real filesystem/subprocess/`notify` behavior belongs there) or routed through a trait-injected `FileSystem` double in the core. For Rust the rule is `no-out-of-module-call`/`no-out-of-module-import`: a unit test may reach only `super::` (the unit) and pure `std` -- no effectful `std::fs`/`std::thread`/`std::env`/`std::time` and no out-of-module first-party import. The fix for a violation is to mock the collaborator (Python: patch it by its dotted path, e.g. `patch("pkg.mod.subprocess.run", ...)`, rather than importing it; TypeScript: `vi.mock("<specifier>")`; Rust: inject a trait double or relocate the effectful test to `tests/`), or, when a dependency is naturally a callable the unit receives, to inject it (DI) -- never to weaken the test.

The scan covers the two native **binding crates** too (#405): `conventions.yml`'s `rust-napi-binding` (`packages/ts/napi`) and `rust-python-binding` (`packages/python`) calls run `colocated-test` + `unit-lint` at each crate root, alongside the core `rust` call. Their **pure** conversion logic is unit-tested inline -- napi's `value_to_js` / `row_event_to_js`, and the pyo3 binding's `row_event_to_plain` (a GIL-free intermediate extracted so the variant->action / field-selection mapping is unit-testable, mirroring napi; `PyRowEvent` is built from it unchanged). The **runtime-coupled** parts (napi `napi::sys` getters / `FnRef`; pyo3 GIL conversions `py_to_value` / `value_to_py` / `value_row_to_py_dict`) stay covered by the binding tier (`tests/integration/binding`), the same #233 split the core uses for effectful code. No exemption: a rust unit test constructing first-party `Value` / `RowEvent` via `super::*` passes `unit-lint`. No `mutation`/`coverage` gate on the binding crates -- those execute the suite and would need the napi/pyo3 build; #405 is the static presence+isolation gates.

`conventions.yml`'s `internals-checks` call (#494/#503) gates the repo-tooling `internals/checks` package the same way: `colocated-test`, `unit-lint`, `integration-lint`, `unit-coverage`, `mutation`, and `e2e-verify` at `internals/checks/src` -- one call per package, `integration-lint` deriving its subjects (`tests/integration/`) from the package root (#515/#417). `tests/integration/` exercises each check's `gate.run()` against real collaborators (real git, real pytest subprocess); `tests/e2e/` spawns the packaged `dirsql-checks` CLI with nothing mocked, gated by `internals/checks/e2e-attestation.json` per the E2E Attestation convention below.

`internals/distcheck` (#520) is the same species of repo-only uv package: the packaging distcheck flows (build → pack → install → run the published artifact), extracted from the former per-package packaging suites. It is a click group (`dirsql-distcheck`) with one subcommand per flow (`dirsql-distcheck python`, `dirsql-distcheck node` -- the node flow drives `npm`/`pnpm` via subprocess from Python; one tested home matters more than harness-language purity), each backed by a `gate.run()` whose effects funnel through an injected `runner` (subprocess) + `FileSystem` seam so the orchestration is unit-testable without a real build. The real flows run in CI via the `distcheck` jobs in `python-test.yml` / `ts-test.yml` (which build the prerequisites first, then invoke `dirsql-distcheck <flow>`); `conventions.yml`'s `internals-distcheck` call gates the package with `colocated-test`, `unit-lint`, `integration-lint`, `unit-coverage`, and `mutation` at `internals/distcheck/src` (a single call -- with #417 live on `@v0`, `integration-lint` derives its subjects from the package root, so no separate integration call is needed). No `e2e-verify`/attestation: the package has no e2e tier, since its `tests/integration/` (each flow's `gate.run()` against real subprocesses) is the outermost tier and the CI distcheck jobs run the real flows directly.

Run it locally before pushing:

```bash
pip install testing-conventions   # CI always uses the latest release
just test-conventions
```

**Exemptions.** The goal is **zero** exemptions, and barrels are no longer an excuse for one. A re-export barrel gets a **colocated test that asserts its public surface** (TS `src/index.ts` ↔ `index.test.ts`, python `dirsql/__init__.py` ↔ `__init___test.py`), exactly as any module would -- it is *tested*, not exempted, so a broken re-export is caught. An `__init__.py` carrying no executable logic is made **truly empty** (0 bytes), which the tool auto-skips with no config entry. A package shell left dead by a feature removal is **deleted**, not parked behind an exemption. When a "barrel" actually holds logic, the fix remains to **extract that logic into colocated-tested modules** (#239). The npm `bin` shim `src/cli/dirsql.ts` is likewise *not* exempt: its error-handling lives in the unit-tested `cli/run-cli.ts`, leaving a trivial `runCli()` shim covered by a mocked distcheck-test.

The exemption count is again **zero**. The three-tier conformance work (#517) restored it: the binding suites moved into the recognized tier `tests/integration/binding/` (#519) and the packaging distcheck flows moved to the `internals/distcheck` package (#520), so nothing trips `unknown-tier` anymore and the temporary `unknown-tier` waivers #518 added are all removed. Before that the last standing entry -- the python barrel-test isolation waiver (`[[python.exempt]] path = "__init___test.py" rules = ["unmocked-collaborator"]`) -- was removed once testing-conventions#382 / PR #384 brought the python `unmocked-collaborator` rule to parity with TS (a bare package-relative import `from . import <names>` in a barrel test now resolves to the SUT and passes). Exemption entries carry a `path` (relative to the scanned source dir for source-file rules, but the derived package root for the suite-tier `unknown-tier` rule), the `rules` waived, and a required `reason`; the CLI **rejects a stale entry whose `path` matches no file**, so it self-cleans. Adding a *new* untested source file fails the gate -- an exemption is never the escape hatch.

### Mutation (testing-conventions)

The rung above coverage is the **`unit mutation`** gate (#235 / epic #231): testing-conventions mutates the source and fails on any **surviving** mutant -- one no unit test caught. Engines: **cosmic-ray** (Python), **Stryker** (TypeScript), **cargo-mutants** (Rust). It is **PR-only and diff-scoped** (`--base <base.sha>...HEAD`): only the lines a PR added/modified are mutated, so each PR's surface stays bounded. A PR that changes no SDK source has nothing to mutate and passes trivially.

The gate reruns the real unit suite per mutant, so it needs the native bindings built. **TypeScript and Rust mutation now run inside the reusable workflow** (`.github/workflows/conventions.yml`): the upstream monorepo primitive -- #277 derives the package root from `source` (the caller input formerly named `path`, renamed upstream; dirsql#577), #279 makes the mutation job install/build from that derived root -- lets the reusable job build the native artifact itself (ts via `build_command: pnpm build` + `rust_toolchain: true`; rust via cargo-mutants, which builds the crate itself). This retired the bespoke `ts-mutation.yml` / `rust-mutation.yml` (#417). The CLI **self-provisions Stryker and cargo-mutants**, so there are no mutation engine deps or config in this repo.

**Python mutation now runs in the reusable workflow too** (`conventions.yml`, `python-sdk` gates: `mutation`): the testing-conventions wheel bundles the cosmic-ray adapter as a runtime dependency, so the reusable mutation job resolves the engine from the same `python_env=uv` (`uv sync`) environment it provisions for coverage — no separate install and no bespoke workflow. This retired `python-mutation.yml` (#426). All three SDKs' `mutation` gates now run inside `conventions.yml`.

Run a language locally (after building its native artifact), against your PR's base:

```bash
# from packages/python (maturin venv active, cosmic-ray installed)
npx -y testing-conventions unit mutation --language python --base origin/main dirsql
# from packages/ts (after pnpm build)
npx -y testing-conventions unit mutation --language typescript --base origin/main src
# from repo root (cargo-mutants installed)
npx -y testing-conventions unit mutation --language rust --base origin/main packages/rust/src
```

**Survivors.** The fix is almost always a **new assertion** that kills the mutant. Only a genuinely *equivalent* or intentionally-defensive mutant is lifted, via a `[[<language>.exempt]]` entry in `testing-conventions.toml` whose `rules` includes `"mutation"` (with a real `path` and a `reason`) -- never weaken a test to make a survivor pass. There are none today.

### Test Boundaries -- What to Mock, What Not To

Unit tests isolate the unit under test. Every dependency that isn't a trivially pure function gets replaced with a fake; production runs the real implementation.

**Mocking is the default.** Use `unittest.mock` / `pytest-mock`'s `mocker` fixture (Python) or `vi.spyOn` / `vi.stubGlobal` / `vi.mock` (TypeScript) to fake out functions, classes, module attributes, and global state for the duration of a test. These tools are scoped (installed on entry, restored on teardown) and keep production code free of test-only seams. Reach for `mock.patch.object` first, both for module imports (`os.execv`, `subprocess.run`, `binary_path`, `is_windows`, `spawnSync`, `die`, `resolveBinary`) and for process/global state (`sys.argv`, `os.environ`, `process.argv`, `process.exit`, `process.stderr`, the system clock, the file system).

**Never use `pytest`'s `monkeypatch` fixture.** Use `unittest.mock.patch.object` / `mocker.patch.object` instead. Functionally similar but `monkeypatch.setattr` conflates module patching with environment mutation and silently encourages leaks.

**Dependency injection is acceptable, not the default.** Reach for a constructor / argument seam only when:

- The dependency is naturally a callable the SUT receives (a callback, an event handler, a strategy object) and DI makes the call graph clearer for non-test reasons.
- Mocking would be substantially more brittle than DI -- e.g. the dependency is invoked from many sites in a tight loop and you want a single typed contract.

For the typical "fake out a stdlib helper / module function" case, mock it instead of refactoring the SUT signature.

**Test-tier rules:**

1. **Unit tests** isolate the SUT and mock every non-pure dependency (or, occasionally, DI it). Coverage at the unit tier should reflect every executable branch.
2. **Integration tests hit the SDK's public API only -- never the CLI.** Mock every third-party dependency (filesystem watchers, network clients, eventual LLM SDKs, SQLite, the filesystem). The CLI is covered by unit (logic) + e2e (full stack), not here.
3. **Binding tests hit the SDK's public API against the real core -- never the CLI, nothing mocked.** Real PyO3/napi binding, real SQLite, real temp-dir filesystem. This is where "the SDK drives real SQLite correctly" behavior (query results, watch events, persistence, extension loading, docs examples) is verified per binding.
4. **E2E tests exercise the CLI and mock nothing.** Real process, real filesystem, real SQLite, real binary. If an e2e test needs a stub, it isn't an e2e test.
5. **Distcheck tests validate the built/packed artifact, not features:** install the published package and run the CLI. A no-mock CI tier (functional publishability); complements the `packaging` gate's file-hygiene check. These flows live in the `internals/distcheck` package (#520), not an SDK-package `tests/` tier.

### E2E Test Policy

E2E tests exercise the CLI and are your primary local feedback mechanism. Run them liberally after significant changes -- they catch issues integration tests miss because integration mocks out SQLite, the filesystem, and (eventually) LLM calls. Do NOT add the e2e suites to CI; CI verifies only the per-package *attestation* that they ran (see *E2E Attestation* below). The no-mock tiers that *do* run in CI are the **binding** tier (`tests/integration/binding/`, the SDK against the real core) and the **distcheck** flows (the `internals/distcheck` package, the functional publishability gate).

See skillet or karat for examples of test organization, fixtures, and pytest-describe patterns.

### E2E Before Push

Agents must run the full e2e suite locally before any `git push` that includes a **substantial code change**, and report the outcome in the PR body. The commands to run differ per environment -- see the active environment file for specifics.

**"Substantial" means any change touching:**
- `packages/rust/**` (Rust core)
- `packages/ts/napi/**` (napi-rs binding crate)
- `packages/python/src/**` (excluding files matching `*_test.py`)
- `packages/ts/src/**` (excluding files matching `*.test.ts` / `*.spec.ts`)
- Any shared SDK runtime code reachable from the above

**Not substantial** (e2e is optional, note "N/A - docs/lint/typo only" in the PR body):
- Docs (`*.md`, `docs/**`, `README*`)
- Lint/format-only changes
- Typo fixes with no behavior change
- Test-only changes (test files themselves)
- CI/workflow config

**PR body requirement:** PRs that include substantial changes must contain this section verbatim (checkboxes filled in):

```markdown
## E2E Verification

- [ ] Ran e2e suites locally for every affected SDK
- [ ] Python SDK e2e: pass / fail / N/A
- [ ] TypeScript SDK e2e: pass / fail / N/A
- [ ] Rust core e2e (if applicable): pass / fail / N/A
- [ ] `packages/python/e2e-attestation.json` refreshed if `packages/python` changed (`just e2e-attest-python`)
- [ ] `packages/ts/e2e-attestation.json` refreshed if `packages/ts` changed (`just e2e-attest-ts`)
- Command(s) run:
- Result summary:
```

For docs/lint/typo-only PRs, include the section with a single line: `N/A - docs/lint/typo only`.

### E2E Attestation

CI does not run the e2e suites -- they need real binaries, and some need live LLM calls -- but it enforces, **per package**, that they *were* run against that package's current code. Each SDK package carries its own attestation at its root -- `packages/python/e2e-attestation.json` and `packages/ts/e2e-attestation.json` -- recording (via [`testing-conventions`](https://github.com/thekevinscott/testing-conventions)) the e2e command, its exit code, and the commit it ran against. The freshness check runs **inside the reusable workflow** (`conventions.yml`, the `python-sdk` / `typescript-sdk` `e2e-verify` gate): `e2e verify "$PACKAGE_ROOT" --scope "$SCAN_PATH" --base "$BASE"` measures freshness over the **`base..HEAD` scoped-source diff**, so a PR that does not touch the SDK source has nothing to verify. It is a **freshness gate, not a test runner** -- no suite, no build, and no LLM run in CI, so it does not violate the E2E Test Policy above. The bespoke `e2e-attestation.yml` this replaced is deleted (whole-hog adoption -- the reusable `e2e-verify` gate owns per-package freshness).

The `--scope` + `--base` diff-scoping makes the gate per-SDK by construction: a change under `packages/python/dirsql` stales only the python attestation, a change under `packages/ts/src` only the ts one, and a PR that does not touch a package's scoped source never verifies it.

**Regenerate the attestation for each package you changed**, as the last commit touching that package before you push. From the repo root:

```bash
just e2e-attest-python   # cd packages/python && testing-conventions e2e attest 'just test-e2e'
just e2e-attest-ts       # cd packages/ts && testing-conventions e2e attest 'pnpm test:e2e'
```

`attest` runs the command, writes `<package>/e2e-attestation.json` naming the current commit, and commits it for you. **The attestation must be the last commit touching that package** -- any later non-attestation commit under the package re-stales it and the gate goes red.

**Multi-package PRs:** because `attest` records `HEAD`, attest each package right after finishing *its* changes (complete + attest python, then complete + attest ts). Attesting both only at the very end leaves whichever you attest second naming the other's attestation commit -- outside its subtree -- which verify rejects.

**Shared-core changes stale both bindings -- CI-enforced via `[e2e].extra_scope`.** The shared Rust core (`packages/rust/src`) is compiled into both bindings but lives in neither binding's subtree, and `e2e verify --scope` requires the scope to be a **descendant of the caller's `path`** -- so a binding caller cannot point its freshness `--scope` at the core. That case is solved by the `e2e verify --extra-scope` / `--exclude` flags (upstream #333): `testing-conventions.toml`'s `[e2e]` block declares `extra_scope = ["packages/rust/src"]`, `detect.py` reads it, and the reusable `e2e-verify` job passes it through -- so **any `packages/rust/src` change now stales BOTH bindings' attestations in CI** and the gate demands re-attestation. Pure config; the former bespoke `e2e_core_freshness.py` gate is not needed (proven: over a `packages/rust/src`-only diff, `e2e verify` is exit 0 without `--extra-scope`, exit 1 with it). **After any shared-core change, re-attest each binding** (`just e2e-attest-python`, `just e2e-attest-ts`) -- CI now enforces this, it is no longer a by-hand-only promise. **`cli`-only** core source (`packages/rust/src/cli/**`, `packages/rust/src/bin/**`) is feature-gated out of the binding *libraries*, but the language packages ship the compiled `dirsql` binary and their e2e suites spawn it -- a CLI change alters what those suites exercise, so it stales the attestations like any other core change (no `exclude` carve-out).

CI installs the latest `testing-conventions` release (unpinned); install it locally before attesting: `pip install testing-conventions`.

### Docs as Spec

**Docs are the canonical specification.** Every documented feature must have a corresponding test. Every test must trace back to a documented feature. If it's not in the docs, don't test it (and question whether it should exist). If it's in the docs, it must have a test.

When adding a feature, the PR must include docs AND tests. When docs change, tests update to match. Agents must run e2e tests locally before pushing substantial changes.

### Changelog and Migrations

**Every PR that touches public-facing SDK code must add a changelog fragment.** This is enforced in CI by the `changelog-gate` check (`internals/checks`), whose implementation mirrors [template-lib](https://github.com/thekevinbot/template-lib)'s reference gate (#566); an unmet gate blocks merge.

The scope: any change to non-test source under `packages/<pkg>/` requires a fragment naming that package. Exempt are test files (`*_test.py`, `*.test.ts` / `*.spec.ts`, anything under `packages/<pkg>/tests/`), the package `CHANGELOG.md` / `MIGRATIONS.md` pointer stubs, and the fragment folders themselves. We err toward requiring entries because the project does not yet strictly follow semver, so the changelog must carry the signal that semver would otherwise provide.

**Fragments are per-package and colocated (#565), so they ship with the package.** Each SDK package (`python`, `ts`, `rust`) owns its own changelog under `packages/<pkg>/changelog.d/`, and a PR adds one fragment per **changed package** -- the fragment lives under the same package whose source changed:

```
packages/<pkg>/changelog.d/YYYY-MM-DD-<slug>.md
```

- `<pkg>` is the package whose public source the PR changed. The Rust core is `rust` (`packages/rust/`), the Python package/binding is `python` (`packages/python/`), the TS package + napi crate is `ts` (`packages/ts/`). The directory identifies the package, so the filename carries no package token. A PR that touches more than one package needs a fragment in each.
- `YYYY-MM-DD` is the UTC merge date; `<slug>` is a short kebab-case description (`2026-07-13-fix-watcher-race.md`).
- The body leads with a Keep a Changelog **category** in bold -- `**Added**` / `**Changed**` / `**Deprecated**` / `**Removed**` / `**Fixed**` / `**Security**` -- then the entry text, exactly as it would read in a changelog. The category lives in the body, **not** the filename.

Fragments are **permanent and append-only** -- nothing is ever assembled back into a root `CHANGELOG.md` and deleted. The root `CHANGELOG.md` / `MIGRATIONS.md` are **frozen** pointer stubs holding only the pre-fragment history (#563/#564); do not edit them. Version history is the `git log --tags` record (the repo tags a release on every merge).

> **Direction of travel is one-way: entries become fragments, never the reverse.** The root `CHANGELOG.md` / `MIGRATIONS.md` are a *closed archive* -- a new entry (even one that documents an already-released change, or a stray fragment left in an old location) is **never** appended, merged, or "folded" into them. The correct home for *any* changelog/migration content that is not already frozen is a fragment under `packages/<pkg>/changelog.d/` (or `migrations.d/`). If you find loose entries in a wrong location -- e.g. the retired **root** `changelog.d/` / `migrations.d/` (the dual-mode dirs that predate the per-package layout, #565) -- **relocate them to the owning package's fragment dir** (renamed to `YYYY-MM-DD-<slug>.md`, body leading with its category), one copy per package the change affected; do **not** move them into the frozen files. Writing into the frozen archive is the mistake the freeze exists to prevent -- if you're adding lines to root `CHANGELOG.md`/`MIGRATIONS.md`, stop: you want a fragment.

**Escape hatch.** If a PR genuinely has no observable change -- a pure refactor, an internal rename, a type-signature tidy with the same runtime -- bypass the gate by adding a `skip-changelog:` line to any commit message on the PR:

```
skip-changelog: <reason>
```

The gate scans raw commit bodies (#566, mirroring template-lib), so the line works from **any** line of any commit -- it need not be a formal git trailer, which removes the blank-line-splits-the-trailer footgun entirely. The reason stays in git history, so the decision is auditable. Use this sparingly; when in doubt, write the changelog fragment.

**A migration fragment is additionally required when a PR:**

- Breaks a public API (signature, name, return type, config key, CLI flag, action input).
- Removes a previously deprecated symbol.
- Changes runtime behavior without changing the API (exit codes, event payloads, on-disk layouts, default values, tag formats).

Purely additive changes and behavior-preserving bug fixes do NOT require a migration entry.

Migration fragments are per-package too, one file per changed package under `packages/<pkg>/migrations.d/YYYY-MM-DD-<slug>.md` (same naming as changelog fragments). Each is a complete entry -- a `### <title>` heading plus the five required subsections:

1. **Summary** -- one paragraph: what broke, which SDKs/call sites, and why.
2. **Required changes** -- table of before/after snippets for every affected surface (config, CLI, action inputs, function signatures, return types).
3. **Deprecations removed** -- previously warned symbols that are now hard errors.
4. **Behavior changes without code changes** -- same API, different runtime behavior.
5. **Verification** -- a concrete dry-run command plus expected output that a consumer can run to confirm the upgrade.

If a subsection does not apply, keep the heading and write `_None._`. Do not omit subsections. The template lives at the bottom of the frozen root `MIGRATIONS.md`.

The frozen root `MIGRATIONS.md` is surfaced on the docs site at `/migrations` via a VitePress include (`docs/migrations.md`). Do not edit the rendered page.

**PR body requirement:** PRs that touch SDK code must contain the following block (checkboxes filled in):

```markdown
## Changelog / Migrations

- [ ] Changelog fragment added under `packages/<pkg>/changelog.d/` for each changed package (or: `skip-changelog` trailer on a commit with reason)
- [ ] Migration fragment added under `packages/<pkg>/migrations.d/` (or: not required -- additive/bugfix only)
```

Orchestrators must block merges of SDK-touching PRs that miss either file when required.

### Cross-SDK Parity (PARITY.md)

`PARITY.md` is a **living document** that tracks API-surface parity across the Python, Rust, and TypeScript SDKs. It must stay current.

On every PR that touches any SDK's public API (`packages/python/dirsql/`, `packages/rust/src/`, or `packages/ts/src/`), the author must:

1. Update `PARITY.md` to reflect the new/changed/removed API surface.
2. Call out in the PR body whether the change is **introducing parity drift** (one SDK gets something the others don't yet) or **restoring parity** (bringing a lagging SDK in line). Drift is allowed but must be intentional and tracked.
3. If drift is introduced, open a follow-up bead for each lagging SDK so the gap is visible.

Orchestrators must block merges of SDK-touching PRs that don't update `PARITY.md`.

### Benchmarks

Run `cargo bench -p dirsql` after significant changes to the Rust codebase. Not in CI -- local only. Covers: SQLite operations, directory scanning, row diffing, glob matching. Use to catch performance regressions before merging.

## Git and GitHub Workflows

### PR Sizing and Issues

- **Every PR is M or smaller.** A change larger than M is broken into a sequence of smaller PRs, each independently reviewable and mergeable. Size by review surface, not raw line count -- a mechanical rename spanning many files can be M, while a subtle core change of far fewer lines may not be.
- **Every PR is accompanied by a GitHub issue it auto-closes.** Put a closing keyword (`Fixes #<n>` / `Closes #<n>`) in the PR body so the issue closes on merge; file the issue first if one does not exist yet.
- **Every PR gets a unique branch name -- NEVER reuse a branch name across PRs, even for follow-up work on the same issue/epic.** After a PR merges (or when starting a new PR), branch a fresh, distinctly-named branch; do **not** re-create the just-merged branch name and stack the next change on it. Reuse is not merely untidy -- it is mechanically unsafe here: the e2e attestation receipt is named after the branch (`packages/<pkg>/e2e-attestations/<branch>.json`), so two PRs on the same branch name write the **same** receipt path. When PR #1 merges, its receipt lands on `main` and the merged branch is pruned (its receipt cleaned up in a later PR); PR #2 on the reused name then *modifies* that same path while `main` *deleted* it -> a **modify/delete merge conflict** that cannot auto-resolve. Worse, reusing the branch hides real code divergence: an unrelated PR that merged to `main` in the meantime can textually auto-merge with your branch into a **compile error** (e.g. `main` adds a call to a helper your branch deleted). A unique branch per PR keeps each receipt path distinct and forces an honest three-way merge against current `main`. (Learned the hard way in epic #601: #602 and #603 both used `claude/tackle-601-kxeykd`.)

### Merge Conflict Resolution

**Merge conflicts on in-flight PRs are the highest priority.** When asked to resolve a merge conflict:

1. Immediately stop other work and focus on the conflict.
2. Pull the latest main/base branch.
3. Resolve the conflict carefully, preserving both intended changes where possible.
4. Commit and push the resolution.
5. Do not proceed with other tasks until the merge conflict is fully resolved and the branch is clean.

### PR Monitoring

Merge gating is via **pr-monitor** (`thekevinscott/pr-monitor@v1`, `.github/workflows/pr-monitor.yml`): the **`CI Gate`** check is an *aggregator*, not a test — it polls every *other* workflow run on the PR and is the single check the merge waits on. Any red among the others turns `CI Gate` red; there is no named-required-checks allowlist in branch protection, so *any* CI/workflow change (renaming/adding/removing a job or check in any `.github/workflows/*.yml`) needs no branch-protection coordination and can't orphan a required check — don't flag that concern.

**When `CI Gate` is red, read its log's last line before acting** — it disambiguates two very different causes:

- `Non-passing runs: ["<Workflow> (failure | startup_failure)"]` → a **real** red in `<Workflow>`; `CI Gate` is only reporting it. A `startup_failure` produces *no separate check-run*, so `CI Gate` can look like the **only** red while masking the actual failure (e.g. a `conventions.yml` input the `@v0` tag no longer defines). Fix/re-run **that** workflow — re-triggering `CI Gate` alone won't help.
- a **timeout** (it waits up to 20 min for slow jobs like Release Precheck) or "still in progress" → a flake. **Re-trigger `CI Gate`** (re-run the PR Monitor run, or push any commit) once the underlying jobs have finished; it then reads them green. This is the common "`CI Gate` is the lone red → re-run → all green" case.

A green `CI Gate` therefore means the whole PR is green.

When monitoring PRs to get them across the finish line (shepherding to green):

1. **Watch for merge conflicts** in addition to CI status. If a PR becomes unmergeable due to conflicts, immediately flag and work to resolve.
2. **Monitor for GPG signing failures** if the repo requires signed commits. Re-sign or re-commit as needed to pass signature checks.
3. Check CI logs for any signing-related errors and address them before merge.
4. Keep the user informed of blockers and resolution status.

### Coverage Floor

Coverage enforcement must stay explicit in CI for each SDK package. All three SDKs are now enforced by [`testing-conventions`](https://github.com/thekevinscott/testing-conventions) `unit coverage`; the per-package floors live in `testing-conventions.toml`:

- **Python / TypeScript** run full tree + a PR-only `--base` changed-lines check, now **inside the reusable workflow** (`conventions.yml`, the `python-sdk` / `typescript-sdk` `unit-coverage` gate) after upstream #284 taught the coverage suite job to install/build from the derived package root (python via `python_env=uv` → `uv sync` builds maturin; ts via `build_command: pnpm build`). This retired the bespoke coverage jobs in `python-test.yml` / `ts-test.yml` (#412). Floors: `[python.coverage]` = `fail_under` / `branch`; `[typescript.coverage]` = `lines` / `branches` / `functions` / `statements` -- both held at the stricter **100%** (every line unit-reachable).
- **Rust core** (#295) is still measured bespoke -- unit-only via `cargo llvm-cov --lib --features cli --branch`, wired into `rust-test.yml`'s coverage job (nightly toolchain for `--branch`, scoped to that job's step alone -- a **permanent** bespoke exception, not pending upstream: the crate-root `rust-toolchain.toml` the reusable job would need breaks release cross-compile, see *Adoption state* above / #437). The CLI now scopes to `--lib` and passes `[rust].features` through (testing-conventions #269/#270/#271). Floors in `[rust.coverage]`: `lines` **94**, `regions` **93**, `functions` **91**, `branch` **75**. #354 first added pure unit tests for the reachable unit-tier gaps (`cli/router.rs`'s `parse_sql_body`, `cli/mod.rs`'s `with_query_timeout`/`From<String>`, `cli/init.rs`'s error `Display` arms), lifting measured unit-only coverage to lines 94.1% / regions 93.2% / functions 91.6% / branch 75.6%; the floors are then set as high as they robustly go -- the highest integer under each actual. `lines`/`regions`/`functions` are above the ≥90% ideal. **`branch` keeps a documented sub-90 floor as an accepted exception** to the ≥90% rule, for two structural reasons. **These floors are pinned near their actuals (<1pt slack) by maintainer request**, so the whole-tree gate goes red on any PR that lowers a dimension -- most fragile for `branch`, where merely adding well-formed unit tests can lower the number (reason (2)); the fix when that happens is to nudge the affected floor down to the new actual, not to chase the dip as a regression. (1) The effectful production branches (filesystem / subprocess / HTTP / `notify`) live in the integration/e2e tier by design (#233): `cli/router.rs`'s async HTTP handlers (0% branch), `cli/init.rs`'s effectful `std::fs::write` success/`--force` arms, and `lib.rs`'s racy-window / watcher / persist error arms are unreachable from a unit test, and covering them re-litigates #233. (2) `--branch` instruments the crate *with its `#[cfg(test)]` modules*, so every `matches!(…, if a && b)` / `assert!(x && y)` guard in a unit test contributes short-circuit sub-condition branches whose False arm is unreachable by construction (the assertion is written to pass) -- `differ.rs` is the clearest case: 100% line/region/function coverage but 58% branch, with every uncovered branch inside a test-module guard, not production code. Because well-formed new unit tests therefore tend to *lower* branch %, the branch floor stays conservative on purpose -- a tight floor would fail legitimate test-adding PRs. The Rust job is **whole-tree only** (no `--base` changed-lines check) for the same reason -- a per-PR changed-lines floor would fail legitimate integration-tier edits.

When work affects more than one SDK package, split the coverage and test work across subagents so each package can be validated independently.

**Coverage floors apply to unit tests only.** Integration, binding, and e2e tests verify behavior through the public surface (binding/e2e as a black box: they spawn subprocesses and hit real filesystems), but they do not contribute to the coverage metric. The line of separation is intentional: the unit-coverage number measures what the library code itself reaches under direct exercise, decoupled from whether the integration scaffolding happens to drag execution through the same lines. `unit coverage` enforces this by construction -- it runs the colocated unit suite only (the Python/TS source dir, not the sibling `tests/` / `test/`), so integration tests never pad the number. A change that refactors implementation without changing behavior should leave integration tests untouched and unit coverage steady; a change that adds untested library code should fail the floor even if integration tests still pass.

This means every covered source file needs in-process unit tests sufficient to hit the floor. **testing-conventions has no whole-file coverage exclusion** -- a file is either unit-tested or its specific uncovered lines are waived with a reason-required line-scoped `[[<lang>.exempt]] rules = ["coverage"]` entry; whole-file waivers are for the presence/lint rules only. So a facade that "needs the native binary" is not an excuse to exclude it -- inject and mock the binding layer and unit-test it (the TS `DirSQL` facade and `dirsql` bin shim are unit-tested this way; today neither binding needs any coverage waiver). Only *test* files are dropped from the metric: for Python via the omit list in `packages/python/pyproject.toml` (`[tool.coverage.run]`, which `coverage run` reads), for TypeScript via the tool's own `**/*.test.*` exclude. Functional exercise of the published launcher happens in the release pipeline's install matrix; the in-CI `packaging` gate additionally asserts such artifacts ship no test files.
