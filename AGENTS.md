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

## Imports

**Prefer relative imports for intra-package references.** Inside a package (Python or TypeScript), use `from .sibling import x` / `import { x } from "./sibling.js"` rather than the absolute `from packagename.sub.sibling import x` / `from "packagename/sub/sibling"`. Relative paths survive renames, signal that the import is internal, and keep cross-cutting refactors (e.g. the `_cli/` → `cli/` rename in #210) from rippling through every import statement. Absolute imports are appropriate when crossing a package boundary or referring to a public re-export.

## File Naming

**TypeScript filenames are dash-case (kebab-case).** Every `.ts` / `.mjs` / `.cjs` / `.json` file under `packages/ts/` uses kebab-case (`load-native-core.ts`, `resolve-binary.test.ts`, `dirsql.config-raises.mjs`); a single lowercase word (`index.ts`, `die.ts`, `main.ts`) is already valid kebab-case and stays. Only filenames follow this rule -- symbols *inside* a file keep their idiomatic `camelCase` / `PascalCase` names (the function in `resolve-binary.ts` is still `resolveBinary`). The convention is enforced for `src/` and `test/` by biome's `style/useFilenamingConvention` rule (`filenameCases: ["kebab-case"]`) and applies package-wide (`tools/`, `test-e2e/`, fixtures) by hand. Python (`snake_case.py`) and Rust (`snake_case.rs`) keep their own ecosystem conventions.

**Python test files use the `_test.py` suffix, not the `test_` prefix** -- a test for `foo.py` is `foo_test.py` (colocated unit tests) or `<feature>_test.py` (integration tests under `tests/integration/`), never `test_foo.py`.

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
- **Integration tests**: `tests/integration/` -- exercise the SDK's **public API** (`DirSQL`, `Table`, `RowEvent`, etc.), with third-party modules (e.g. the `notify` filesystem watcher, network calls, future LLM clients) replaced by **fixture-injected fakes**. Run in CI.
- **E2E tests**: `tests/e2e/` -- real filesystem, real SQLite, real LLM calls, real published-artifact install (wheel / npm tarball). **No mocks, no fakes, no monkeypatching.** Heavy use of pytest fixtures. **NOT run in CI.** Artifact *hygiene* -- that no test files ship in the built `.whl` / `.tgz` / `.crate` -- is enforced in CI by the **`packaging` gate** (testing-conventions; `.github/workflows/packaging.yml`); *functional* publishability (a real install + run of the published artifact) is covered by the release pipeline's cross-triple matrix, not an in-CI smoke test.

### Enforcing Colocation (testing-conventions)

The Python/TypeScript/Rust colocation rule above is enforced as a **blocking CI gate** by [`testing-conventions`](https://github.com/thekevinscott/testing-conventions), a config-driven CLI that scans each SDK's source tree and fails on any source file lacking a colocated unit test (for Rust, an inline `#[cfg(test)]` module). The wiring lives in `.github/workflows/testing-conventions.yml` (it pins the CLI version and runs the per-language `unit colocated-test` presence scans, plus a PR-only `--base` co-change check for Python/TypeScript that fails when a modified source's colocated test did not change alongside it) and `testing-conventions.toml` (the exempt list).

The same workflow also runs `unit lint` -- the **isolation** rule: a unit test must mock every collaborator (it must not import an un-mocked one), so the test exercises only the unit under test. It is wired for **all three SDKs** (#233 / epic #231): Python and TypeScript first, then Rust once its effectful-std unit tests were either moved to the integration tier (real filesystem/subprocess/`notify` behavior belongs there) or routed through a trait-injected `FileSystem` double in the core. For Rust the rule is `no-out-of-module-call`/`no-out-of-module-import`: a unit test may reach only `super::` (the unit) and pure `std` -- no effectful `std::fs`/`std::thread`/`std::env`/`std::time` and no out-of-module first-party import. The fix for a violation is to mock the collaborator (Python: patch it by its dotted path, e.g. `patch("pkg.mod.subprocess.run", ...)`, rather than importing it; TypeScript: `vi.mock("<specifier>")`; Rust: inject a trait double or relocate the effectful test to `tests/`), or, when a dependency is naturally a callable the unit receives, to inject it (DI) -- never to weaken the test.

Run it locally before pushing:

```bash
pip install "testing-conventions==<version>"   # version pinned in the workflow
just test-conventions
```

**Exemptions.** Genuine entry shims that are deliberately not unit-tested are declared in `testing-conventions.toml` as `[[python.exempt]]` / `[[typescript.exempt]]` entries, each carrying a `path` (relative to the scanned source dir), the `rules` it waives (`colocated-test`, plus `co-change` for the testless TS entries whose source can still legitimately change -- e.g. a barrel whose imports move in a rename -- with no sibling test to co-update), and a required `reason`. Today that covers the package barrels (`dirsql/__init__.py`, `src/index.ts`, `src/cli/interpret/index.ts`), the docstring-only CLI packages (`dirsql/cli/__init__.py`, `dirsql/cli/interpret/__init__.py`), and the npm `bin` launcher (`src/cli/dirsql.ts`, a trivial entry shim that invokes `main()` at load). Keep the list minimal and in lockstep with the coverage-omit configs it mirrors (`packages/python/pyproject.toml`, `packages/ts/vitest.config.ts`); the CLI **rejects a stale exempt entry whose `path` matches no file**, so remove an entry the moment its file gains a real colocated test or is deleted. Adding a *new* untested source file fails the gate -- exemptions are the rare, documented exception, not the escape hatch.

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
2. **Integration tests hit the SDK's public API.** They may use fakes for third-party modules (filesystem watchers, network clients, eventual LLM SDKs) -- but inject them through the public API or a fixture, not by patching the production module's attributes.
3. **E2E tests mock nothing.** Real filesystem, real SQLite, real binary, real install. If an e2e test needs a stub, it isn't an e2e test.

### E2E Test Policy

E2E tests are your primary feedback mechanism. Run them liberally after significant changes -- they catch issues that integration tests miss because integration tests mock out SQLite and (eventually) LLM calls. But do NOT add them to CI workflows. They are a local development tool.

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
- Command(s) run:
- Result summary:
```

For docs/lint/typo-only PRs, include the section with a single line: `N/A - docs/lint/typo only`.

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

Coverage enforcement must stay explicit in CI for each SDK package:

- Rust core coverage must stay at 90% or higher.
- Python SDK coverage must stay at 90% or higher.
- TypeScript SDK coverage must stay at 90% or higher.

When work affects more than one SDK package, split the coverage and test work across subagents so each package can be validated independently.

**Coverage floors apply to unit tests only.** Integration tests (and e2e tests) verify end-to-end behavior as a black box; they spawn subprocesses, hit real filesystems, and exercise the public API surface, but they do not contribute to the coverage metric. The line of separation is intentional: the unit-coverage number measures what the library code itself reaches under direct exercise, decoupled from whether the integration scaffolding happens to drag execution through the same lines. A change that refactors implementation without changing behavior should leave integration tests untouched and unit coverage steady; a change that adds untested library code should fail the floor even if integration tests still pass.

This means every covered source file needs in-process unit tests sufficient to hit the floor. If a file's only meaningful exercise path is via subprocess (e.g. the `os.execv` / `spawnSync` handoff in the CLI launchers), call that out explicitly in the coverage config's omit/exclude list with a comment. Functional exercise of the published launcher happens in the release pipeline's install matrix rather than the unit-coverage metric; the in-CI `packaging` gate additionally asserts such artifacts ship no test files.
