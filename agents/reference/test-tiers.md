# Test Tiers

How the four test tiers divide the work, what each may mock, and where each lives. The gates that enforce colocation, coverage and mutation are a separate concern -- see `testing-gates.md`.

## Red/Green Development

Follow **red/green** (test-first) methodology:

1. **Write red integration AND e2e tests first** -- it must capture the desired behavior
2. **Run it and confirm it fails (RED)** -- do NOT proceed until the test turns red reliably. A test that passes before implementation proves nothing.
3. **Push the failing test as its own commit and confirm CI goes red for the right reason** -- the failing test must be committed and pushed on its own, and the CI run for that commit must be observed failing before any implementation is written. Local RED is not enough; CI RED is the gate. The failure must be *relevant*: CI must fail specifically because the new test's assertions are unmet, not because of an unrelated flake, a compile error elsewhere, a pre-existing failure, or an infrastructure hiccup. Open the failing job, confirm the new test is the thing that failed, and confirm the failure message matches the behavior the test asserts. A green run, a skipped run, or a red run that fails for any other reason all mean the test is not proving what it must -- fix the test and re-confirm, do not proceed.
4. **Make the minimal change to pass (GREEN)** -- only then write the implementation, committed and pushed separately so CI flips from red to green.
5. Refactor if needed, keeping tests green

**Removals are exempt.** Red/green applies to adding or changing behavior. When a change *removes* functionality, do not write red tests enforcing the absence of the removed behavior, and the pushed-RED-CI gate does not apply: delete or update the tests that covered the removed behavior, keep the suite green, and commit implementation + test updates normally. If the removal introduces genuinely new observable behavior (e.g. a new error message when a retired config key is used), that new behavior follows normal red/green.

## TDD Order: Outside-In

Tests are written **before** implementation, starting from the outermost layer:

1. **Integration test first** -- proves the feature works from the consumer's perspective
2. **Unit tests** -- written as you implement each module

A feature is not done until integration tests pass and cover the new functionality.

## When to Write What

**Does the commit change the public-facing API?**
- Yes -> **integration test required**, plus unit tests as you go
- No -> Check if adequate integration coverage already exists:
  - Adequate -> unit tests only
  - Gaps -> add the missing integration tests, plus unit tests

**Always write unit tests.** The question is whether you also need integration tests.

## Test Locations

- **Unit tests**: Colocated with source
  - Python: `foo.py` -> `foo_test.py` in same directory
  - TypeScript: `foo.ts` -> `foo.test.ts` in same directory
  - Rust: inline `#[cfg(test)]` module at bottom of each source file
- **Integration tests**: `tests/integration/` -- exercise the **SDK** public API (`DirSQL`, `Table`, `RowEvent`, etc.) **only, never the CLI**. Two subdirectories, run as two CI jobs:
  - `tests/integration/hermetic/` -- **every** third-party dependency mocked (the `notify` watcher, network, future LLM clients, and **SQLite and the filesystem** too -- hermetic since #289: Python patches the `_RustDirSQL` core boundary via `unittest.mock`, TypeScript `vi.mock`s `src/core.ts`). Needs no native build.
  - `tests/integration/binding/` (#289) -- the SDK public API against the **real core** (PyO3 / napi binding, real SQLite, real temp-dir filesystem). Proves the SDK↔core marshaling and real query/watch/persist behavior the hermetic subdir mocks out -- coverage the CLI e2e suites cannot provide at the granularity the binding tier needs -- though since #721 the CLI *does* cross a binding (the launchers call `run_cli` in-process), so the e2e suites now exercise that path too. Its CI job builds the native artifact (maturin / napi + cargo). This is the only real-core coverage, so unlike e2e it **runs on every PR** -- upstream's integration definition (first-party code runs for real; mocking the outside world is *permitted*, not required) fits it as-is.
  Both **run in CI**. Rust has no binding subdir: it *is* the core, so `packages/rust/tests/` remains its integration tier.
- **E2E tests**: `tests/e2e/` -- exercise the **CLI** only (the launcher, which since #721 runs `run_cli` in-process through the binding rather than spawning a bundled binary) with **nothing mocked**. That change makes the "nothing mocked" claim *more* faithful, not less: the suites now run the exact path a user runs. **No mocks, no fakes, no monkeypatching. NOT run in CI** -- CI verifies only the per-package *attestation* that they ran (see *E2E Attestation*).
- **Distcheck tests**: **not** an SDK-package tier -- the *functional* publishability flows (build, pack, install, and run the published artifact) live in the `internals/distcheck` package (#520), which itself follows the three-tier layout. **Run in CI** via that package's `dirsql-distcheck python` / `dirsql-distcheck node` entry points (the `distcheck` jobs in `dirsql-python-ci.yml` / `dirsql-typescript-ci.yml`). Distinct from the **`packaging` gate** (testing-conventions, run from each language's CI workflow), which only asserts no test files *ship* in the built artifact and never installs or runs it.

## Test Boundaries -- What to Mock, What Not To

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

## E2E Test Policy

E2E tests exercise the CLI and are your primary local feedback mechanism. Run them liberally after significant changes -- they catch issues integration tests miss because integration mocks out SQLite, the filesystem, and (eventually) LLM calls. Do NOT add the e2e suites to CI; CI verifies only the per-package *attestation* that they ran (see *E2E Attestation* below). The no-mock tiers that *do* run in CI are the **binding** tier (`tests/integration/binding/`, the SDK against the real core) and the **distcheck** flows (the `internals/distcheck` package, the functional publishability gate).

See skillet or karat for examples of test organization, fixtures, and pytest-describe patterns.

## Docs as Reference

**Docs are the canonical description of intended behavior** -- the human source of truth for what the product does. They carry **no** test obligation: there is no rule that every documented feature must have a test, that every test must trace back to a doc, or that tests update whenever docs change. Docs themselves are not tested.

Product *behavior* stays covered by the normal unit / integration / binding / e2e tiers -- that is unchanged. When adding a feature, the PR still includes docs (the human description) and whatever behavior tests the change warrants; the two are simply no longer coupled by a gate.
