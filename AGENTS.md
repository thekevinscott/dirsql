# dirsql Development

In your responses, strive for brevity. As concise as possible.

## Architecture

All architectural decisions and constraints (including cross-language parity rules, the one-implementation principle, and SDK design) are in `ARCHITECTURE.md`. Do NOT put architectural information in this file -- AGENTS.md is for workflow and process only.

@agents/build/environment.md

## Scratch Files

Write scratch/temporary files to `/tmp` instead of asking permission. Use unique filenames to avoid collisions with other sessions.
Temporary scripts, including Node or shell helpers, must also be written to `/tmp` and executed from there.

## Shell Commands

**Do not chain commands** with `;`, `&&`, or `||`. Chained commands break the per-command permission model -- each command must be evaluated separately, and chaining forces a single bulk approval (or prompt) for the whole pipeline. Run each command as its own call.

Exceptions: piping (`|`) is fine when it's genuinely one logical operation (e.g., `cmd | jq`). Heredocs (`cat <<EOF`) are fine. `cd path && cmd` is NOT fine -- use `cd` as a separate call (or pass absolute paths).

## CI Workflows

**CI logic lives in scripts, not workflow YAML.** `run:` / `github-script` steps stay trivial glue -- check out, set up a toolchain, invoke one command. Anything with iteration, `case` dispatch, conditionals, or text-munging moves to a script under `.github/scripts/`, invoked as a one-liner, and carries **colocated unit tests** (the same testing-conventions standard as the rest of the tree -- `foo.py` ↔ `foo_test.py`). Those tests run under a 100% coverage floor in `.github/workflows/gha-scripts.yml`. Inline workflow logic is untestable, un-runnable locally, and silently duplicated across runners; a script is none of those.

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
- **Integration tests**: `tests/integration/` -- exercise the **SDK** public API (`DirSQL`, `Table`, `RowEvent`, etc.) **only, never the CLI**, with **every** third-party dependency mocked (the `notify` watcher, network, future LLM clients, and **SQLite and the filesystem** too -- hermetic since #289: Python patches the `_RustDirSQL` core boundary via `unittest.mock`, TypeScript `vi.mock`s `src/core.ts`). Run in CI.
- **Binding tests**: `tests/binding/` (#289) -- exercise the **SDK** public API against the **real core** (PyO3 / napi binding, real SQLite, real temp-dir filesystem), **never the CLI**. This tier proves the SDK↔core marshaling and real query/watch/persist behavior that the hermetic integration tier mocks out -- coverage the CLI e2e suites cannot provide, since the CLI is a pure Rust binary that never crosses a binding. **Run in CI.** Rust has no binding tier: it *is* the core, so `packages/rust/tests/` remains its integration tier.
- **E2E tests**: `tests/e2e/` -- exercise the **CLI** only (the `dirsql` binary, the `dirsql interpret` subprocess, the launcher) with **nothing mocked**. **No mocks, no fakes, no monkeypatching. NOT run in CI** -- CI verifies only the per-package *attestation* that they ran (see *E2E Attestation*).
- **Smoke tests**: `tests/smoke/` -- *functional* publishability: build, pack, install, and run the published artifact (`build.test.ts`, `build_test.py`). **Run in CI.** Distinct from the **`packaging` gate** (testing-conventions; `.github/workflows/packaging.yml`), which only asserts no test files *ship* in the `.whl` / `.tgz` / `.crate` and never installs or runs it.

### Enforcing Colocation (testing-conventions)

The Python/TypeScript/Rust colocation rule above is enforced as a **blocking CI gate** by [`testing-conventions`](https://github.com/thekevinscott/testing-conventions), a config-driven CLI that scans each SDK's source tree and fails on any source file lacking a colocated unit test (for Rust, an inline `#[cfg(test)]` module). The wiring lives in `.github/workflows/testing-conventions.yml` (it pins the CLI version and runs the per-language `unit colocated-test` presence scans, plus a PR-only `--base` co-change check for Python/TypeScript that fails when a modified source's colocated test did not change alongside it) and `testing-conventions.toml` (the exempt list).

The same workflow also runs `unit lint` -- the **isolation** rule: a unit test must mock every collaborator (it must not import an un-mocked one), so the test exercises only the unit under test. It is wired for **all three SDKs** (#233 / epic #231): Python and TypeScript first, then Rust once its effectful-std unit tests were either moved to the integration tier (real filesystem/subprocess/`notify` behavior belongs there) or routed through a trait-injected `FileSystem` double in the core. For Rust the rule is `no-out-of-module-call`/`no-out-of-module-import`: a unit test may reach only `super::` (the unit) and pure `std` -- no effectful `std::fs`/`std::thread`/`std::env`/`std::time` and no out-of-module first-party import. The fix for a violation is to mock the collaborator (Python: patch it by its dotted path, e.g. `patch("pkg.mod.subprocess.run", ...)`, rather than importing it; TypeScript: `vi.mock("<specifier>")`; Rust: inject a trait double or relocate the effectful test to `tests/`), or, when a dependency is naturally a callable the unit receives, to inject it (DI) -- never to weaken the test.

Run it locally before pushing:

```bash
pip install testing-conventions   # CI always uses the latest release
just test-conventions
```

**Exemptions.** The principle is narrow: a file is exempt only if it is a **true barrel** (a re-export–only module -- `index.ts` / package `__init__.py`) or an init carrying no executable logic; anything with real code gets a colocated unit test, never an exemption (#239). When a file is exempt as a "barrel" but actually holds logic, the fix is to **extract that logic into colocated-tested modules** until what remains is a genuine barrel -- not to test the barrel. Exemptions are declared in `testing-conventions.toml` as `[[python.exempt]]` / `[[typescript.exempt]]` entries, each carrying a `path` (relative to the scanned source dir), the `rules` it waives (`colocated-test`, plus `co-change` for the testless barrels whose imports can move in a rename with no sibling test to co-update), and a required `reason`. Today that covers the public package barrel (`dirsql/__init__.py`), the docstring-only CLI packages (`dirsql/cli/__init__.py`, `dirsql/cli/interpret/__init__.py`), and the two TS re-export barrels (`src/index.ts` -- whose Table/DirSQL/parseTableName/core logic was extracted into `table.ts`/`core.ts`/`parse-table-name.ts`/`dirsql.ts` -- and `src/cli/interpret/index.ts`). The npm `bin` shim `src/cli/dirsql.ts` is *not* exempt: its error-handling logic lives in the unit-tested `cli/run-cli.ts`, leaving a trivial `runCli()` shim covered by a mocked smoke-test. Keep the list minimal and in lockstep with the coverage-omit configs it mirrors (`packages/python/pyproject.toml`, `packages/ts/vitest.config.ts`); the CLI **rejects a stale exempt entry whose `path` matches no file**, so remove an entry the moment its file gains a real colocated test or is deleted. Adding a *new* untested source file fails the gate -- exemptions are the rare, documented exception, not the escape hatch.

### Mutation (testing-conventions)

The rung above coverage is the **`unit mutation`** gate (#235 / epic #231): testing-conventions mutates the source and fails on any **surviving** mutant -- one no unit test caught. Engines: **cosmic-ray** (Python), **Stryker** (TypeScript), **cargo-mutants** (Rust). It is **PR-only and diff-scoped** (`--base <base.sha>...HEAD`): only the lines a PR added/modified are mutated, so each PR's surface stays bounded. A PR that changes no SDK source has nothing to mutate and passes trivially.

The gate reruns the real unit suite per mutant, so it needs the native bindings built -- and the testing-conventions reusable workflow has no build step (epic #231's constraint). So each language gets its own build-capable workflow that builds the artifact and drives the CLI (invoked unpinned via `npx -y testing-conventions`, so CI always runs the latest): `.github/workflows/python-mutation.yml` (`maturin develop`), `ts-mutation.yml` (`pnpm build`), `rust-mutation.yml` (cargo). The CLI **self-provisions Stryker and cargo-mutants**, so there are no mutation engine deps or config in this repo; only **cosmic-ray** plus the **testing-conventions pip package** (which ships the cosmic-ray adapter the CLI spawns as `python3 -m testing_conventions.mutation.main`) are installed by the job (into the maturin venv, so the `python3 -m pytest` baseline resolves the built `_dirsql`). Adopting the reusable workflow instead is blocked upstream (job scoping + a second-toolchain hook -- see #240).

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
5. **Smoke tests validate the built/packed artifact, not features:** install the published package and run the CLI. A no-mock CI tier (functional publishability); complements `packaging.yml`'s file-hygiene check.

### E2E Test Policy

E2E tests exercise the CLI and are your primary local feedback mechanism. Run them liberally after significant changes -- they catch issues integration tests miss because integration mocks out SQLite, the filesystem, and (eventually) LLM calls. Do NOT add the e2e suites to CI; CI verifies only the per-package *attestation* that they ran (see *E2E Attestation* below). The no-mock tiers that *do* run in CI are the **binding** tier (`tests/binding/`, the SDK against the real core) and the **smoke** tier (`tests/smoke/`, the functional publishability gate).

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

CI does not run the e2e suites -- they need real binaries, and some need live LLM calls -- but it enforces, **per package**, that they *were* run against that package's current code. Each SDK package carries its own attestation at its root -- `packages/python/e2e-attestation.json` and `packages/ts/e2e-attestation.json` -- recording (via [`testing-conventions`](https://github.com/thekevinscott/testing-conventions)) the e2e command, its exit code, and the commit it ran against. `.github/workflows/e2e-attestation.yml` runs `testing-conventions e2e verify` **inside each package the PR changed**; verify walks history *scoped to that package's subtree* and fails if the package's attestation does not name the latest commit touching it. It is a **freshness gate, not a test runner** -- no suite, no build, and no LLM run in CI, so it does not violate the E2E Test Policy above.

The subtree scoping makes the gate per-SDK by construction: a change under `packages/python` stales only the python attestation, a change under `packages/ts` only the ts one, and a PR that does not touch a package never runs that package's verify.

**Regenerate the attestation for each package you changed**, as the last commit touching that package before you push. From the repo root:

```bash
just e2e-attest-python   # cd packages/python && testing-conventions e2e attest 'just test-e2e'
just e2e-attest-ts       # cd packages/ts && testing-conventions e2e attest 'pnpm test:e2e'
```

`attest` runs the command, writes `<package>/e2e-attestation.json` naming the current commit, and commits it for you. **The attestation must be the last commit touching that package** -- any later non-attestation commit under the package re-stales it and the gate goes red.

**Multi-package PRs:** because `attest` records `HEAD`, attest each package right after finishing *its* changes (complete + attest python, then complete + attest ts). Attesting both only at the very end leaves whichever you attest second naming the other's attestation commit -- outside its subtree -- which verify rejects.

**Shared-core changes stale both bindings.** The shared Rust core (`packages/rust`) is compiled into both bindings but lives in neither subtree, so `testing-conventions e2e verify` (which walks only the binding subtree) cannot see a core change. A dedicated gate closes that blind spot (#337): `.github/workflows/e2e-attestation.yml` runs `.github/scripts/e2e_core_freshness.py`, which fails when a **non-`cli`** change under `packages/rust/src/**` is not yet reflected in each binding's attestation (the changed core commit must be an ancestor of, or equal to, the attested commit). So a binding-linked core change stales **both** `packages/python/e2e-attestation.json` and `packages/ts/e2e-attestation.json` -- re-attest each binding after the core change. **`cli`-only** core source (`packages/rust/src/cli/**`, `packages/rust/src/bin/**`) is feature-gated behind the `cli` Cargo feature and never compiled into the bindings, so it is excluded from the staling set and does not force re-attestation (#328 was exactly this shape).

CI installs the latest `testing-conventions` release (unpinned); install it locally before attesting: `pip install testing-conventions`.

### Docs as Spec

**Docs are the canonical specification.** Every documented feature must have a corresponding test. Every test must trace back to a documented feature. If it's not in the docs, don't test it (and question whether it should exist). If it's in the docs, it must have a test.

When adding a feature, the PR must include docs AND tests. When docs change, tests update to match. Agents must run e2e tests locally before pushing substantial changes.

### Changelog and Migrations

**Every PR that touches public-facing SDK code must update `CHANGELOG.md`.** This is enforced in CI by `.github/workflows/changelog-check.yml`; an unmet gate blocks merge.

The scope is intentionally broad -- any change under SDK source (Rust core, Python/TS packages, binding crates, or top-level `Cargo.toml` / `Cargo.lock`) requires a changelog entry, excluding test-only files. We err toward requiring entries because the project does not yet strictly follow semver, so the changelog must carry the signal that semver would otherwise provide.

**Escape hatch.** If a PR genuinely has no observable change -- a pure refactor, an internal rename, a type-signature tidy with the same runtime -- bypass the gate by adding a trailer to any commit in the PR:

```
skip-changelog: <reason>
```

The reason is logged to CI and stays in git history, so the decision is auditable. Use this sparingly; when in doubt, write the changelog entry.

Every entry goes under `## [Unreleased]`, categorized per [Keep a Changelog](https://keepachangelog.com/en/1.1.0/): `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`.

**`MIGRATIONS.md` is additionally required when a PR:**

- Breaks a public API (signature, name, return type, config key, CLI flag, action input).
- Removes a previously deprecated symbol.
- Changes runtime behavior without changing the API (exit codes, event payloads, on-disk layouts, default values, tag formats).

Purely additive changes and behavior-preserving bug fixes do NOT require a migration entry.

Migration entries live under `## [Unreleased]` in `MIGRATIONS.md` and must follow the template at the bottom of that file. Every entry has five required subsections:

1. **Summary** -- one paragraph: what broke, which SDKs/call sites, and why.
2. **Required changes** -- table of before/after snippets for every affected surface (config, CLI, action inputs, function signatures, return types).
3. **Deprecations removed** -- previously warned symbols that are now hard errors.
4. **Behavior changes without code changes** -- same API, different runtime behavior.
5. **Verification** -- a concrete dry-run command plus expected output that a consumer can run to confirm the upgrade.

If a subsection does not apply, keep the heading and write `_None._`. Do not omit subsections.

`MIGRATIONS.md` is the source of truth and is surfaced on the docs site at `/migrations` via a VitePress include (`docs/migrations.md`). Do not edit the rendered page; edit the root file and the docs site picks up the change on the next build.

**PR body requirement:** PRs that touch SDK code must contain the following block (checkboxes filled in):

```markdown
## Changelog / Migrations

- [ ] `CHANGELOG.md` updated under `## [Unreleased]` (or: `skip-changelog` trailer on a commit with reason)
- [ ] `MIGRATIONS.md` updated (or: not required -- additive/bugfix only)
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

### Coverage Floor

Coverage enforcement must stay explicit in CI for each SDK package, at 90% or higher:

- **Python / TypeScript** are enforced by [`testing-conventions`](https://github.com/thekevinscott/testing-conventions) `unit coverage` (full tree + a PR-only `--base` changed-lines check), wired into `python-test.yml` / `ts-test.yml`. The per-package floors live in `testing-conventions.toml` (`[python.coverage]` = `fail_under` / `branch`; `[typescript.coverage]` = `lines` / `branches` / `functions` / `statements`) and are currently held at the stricter **100%**.
- **Rust core** keeps its bespoke `cargo llvm-cov` job in `rust-test.yml` for now: the testing-conventions CLI cannot yet measure it unit-only (it runs the integration tests too and can't enable the `cli` feature). Migration is tracked in #295.

When work affects more than one SDK package, split the coverage and test work across subagents so each package can be validated independently.

**Coverage floors apply to unit tests only.** Integration, binding, and e2e tests verify behavior through the public surface (binding/e2e as a black box: they spawn subprocesses and hit real filesystems), but they do not contribute to the coverage metric. The line of separation is intentional: the unit-coverage number measures what the library code itself reaches under direct exercise, decoupled from whether the integration scaffolding happens to drag execution through the same lines. `unit coverage` enforces this by construction -- it runs the colocated unit suite only (the Python/TS source dir, not the sibling `tests/` / `test/`), so integration tests never pad the number. A change that refactors implementation without changing behavior should leave integration tests untouched and unit coverage steady; a change that adds untested library code should fail the floor even if integration tests still pass.

This means every covered source file needs in-process unit tests sufficient to hit the floor. **testing-conventions has no whole-file coverage exclusion** -- a file is either unit-tested or its specific uncovered lines are waived with a reason-required line-scoped `[[<lang>.exempt]] rules = ["coverage"]` entry; whole-file waivers are for the presence/lint rules only. So a facade that "needs the native binary" is not an excuse to exclude it -- inject and mock the binding layer and unit-test it (the TS `DirSQL` facade and `dirsql` bin shim are unit-tested this way; today neither binding needs any coverage waiver). Only *test* files are dropped from the metric: for Python via the omit list in `packages/python/pyproject.toml` (`[tool.coverage.run]`, which `coverage run` reads), for TypeScript via the tool's own `**/*.test.*` exclude. Functional exercise of the published launcher happens in the release pipeline's install matrix; the in-CI `packaging` gate additionally asserts such artifacts ship no test files.
