# dirsql Development

In your responses, strive for brevity. As concise as possible.

## Architecture

All architectural decisions and constraints (including cross-language parity rules, the one-implementation principle, and SDK design) are in `ARCHITECTURE.md`. Do NOT put architectural information in this file -- AGENTS.md is for workflow and process only.

@agents/build/environment.md

## Reference Docs

Deep operational references live in `agents/reference/` and are NOT auto-loaded (this file must stay under Claude Code's 40k-char instruction limit). Each summarized section below names its reference file -- **read it before working in that area**: reusable-workflow gate debugging (`testing-conventions-ci.md`), colocation/mutation/coverage gates (`testing-gates.md`), e2e attestation (`e2e-attestation.md`), changelog/migration fragments (`changelog-migrations.md`), merge gating (`pr-monitor.md`).

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

## Dependencies

**Never `uv pip install` (or `pnpm link`) into a package's venv during development.** Add the dependency to the manifest (`[project].dependencies`, or `[dependency-groups].dev` for a test-only one) and run `uv sync`. `uv pip install` populates the venv without declaring anything, leaving it **strictly more capable than any real install** -- so the import resolves locally in an environment no user will ever have. This is not a gate-coverage problem: every gate passes, because they all run inside the drifted venv. In #777 one undeclared `bin_shim` import sailed past 108 unit tests, 100% coverage, 27 e2e tests and a clean `ty`, then turned **seven CI jobs red** on a clean resolve (#782).

Two backstops, both in `just preflight`: `uv sync` per python root (which *removes* whatever a `uv pip install` left behind) and `dirsql-checks declared-deps <source>`, which asserts every third-party import in a tree resolves to a distribution its manifest declares. The convention is the durable fix; the gates are the safety net.

## Comments

Default to no comments. Only add one when the WHY is non-obvious -- a hidden constraint, an invariant, a workaround, something that would surprise a reader. Never write archaeology: no issue/PR references, no "added for the X flow" / "used by Y", no restating what adjacent code already says, no reviewer-directed justification. That belongs in the commit message and PR description, not the file -- it rots as the codebase evolves and the file is never re-read once merged. See #445 (trimmed exactly this style repo-wide) and CHANGELOG.md's entry for it.

## CI Workflows

**Every CI check emits actionable fix instructions on failure.** A failing check must tell the contributor exactly what to change -- the file, command, or trailer to add or edit -- not merely which rule was violated. When a check can detect a *near-miss* (a fix was attempted but malformed), it names the specific defect and how to correct it rather than falling through to a generic "not satisfied" message (e.g. the `changelog-gate` names a fragment file whose name breaks the `YYYY-MM-DD-<slug>.md` convention -- pointing at the exact file -- and its "no fragment" error prints the exact path to add; dirsql#566).

**CI logic lives in scripts, not workflow YAML.** `run:` / `github-script` steps stay trivial glue -- check out, set up a toolchain, invoke one command. Anything with iteration, `case` dispatch, conditionals, or text-munging moves to a check in the `internals/checks` uv package (a click group, one subcommand per check -- see `internals/checks/src/checks/`), invoked as a one-liner (`uv run --project internals/checks dirsql-checks <check>`), and carries **colocated unit tests** (the same testing-conventions standard as the rest of the tree -- `foo.py` ↔ `foo_test.py`). Those tests run under `internals-checks-ci.yml`'s `internals-checks` job (`unit-coverage` enforces a 100% floor; full gate list in `agents/reference/testing-gates.md`). Inline workflow logic is untestable, un-runnable locally, and silently duplicated across runners; a script is none of those.

### Reusable-workflow gates (testing-conventions)

Six per-domain workflows call the `testing-conventions` reusable workflow at the **moving tag `@v0`** -- `dirsql-python-ci.yml`, `dirsql-typescript-ci.yml`, `dirsql-rust-ci.yml`, `internals-checks-ci.yml`, `internals-distcheck-ci.yml`, `plugin-dirsql-embedding-ci.yml` (#861 split the single `conventions.yml` into these, one per domain, so path filters could triage per lane). The essentials: a removed/renamed input startup-fails the whole *calling* workflow on `main` and every PR (0 jobs, no job logs -- WebFetch the run's `html_url` and read the annotation first, never hypothesize), and since every caller passes the same inputs, one bad input reds all six; an unknown `testing-conventions.toml` key fails EVERY gate; read upstream behavior at a pinned sha and trust its `MIGRATIONS.md` over probing; a stale e2e attestation after a squash merge is ours to re-attest, never a tool bug; never retire a bespoke gate before a green proof. Full operational lore, the per-language gate adoption map, and the two permanent exceptions (Rust unit-coverage stays bespoke; Rust has no e2e-verify): `agents/reference/testing-conventions-ci.md`.

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

**Removals are exempt.** Red/green applies to adding or changing behavior. When a change *removes* functionality, do not write red tests enforcing the absence of the removed behavior, and the pushed-RED-CI gate does not apply: delete or update the tests that covered the removed behavior, keep the suite green, and commit implementation + test updates normally. If the removal introduces genuinely new observable behavior (e.g. a new error message when a retired config key is used), that new behavior follows normal red/green.

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
  - `tests/integration/binding/` (#289) -- the SDK public API against the **real core** (PyO3 / napi binding, real SQLite, real temp-dir filesystem). Proves the SDK↔core marshaling and real query/watch/persist behavior the hermetic subdir mocks out -- coverage the CLI e2e suites cannot provide at the granularity the binding tier needs -- though since #721 the CLI *does* cross a binding (the launchers call `run_cli` in-process), so the e2e suites now exercise that path too. Its CI job builds the native artifact (maturin / napi + cargo). This is the only real-core coverage, so unlike e2e it **runs on every PR** -- upstream's integration definition (first-party code runs for real; mocking the outside world is *permitted*, not required) fits it as-is.
  Both **run in CI**. Rust has no binding subdir: it *is* the core, so `packages/rust/tests/` remains its integration tier.
- **E2E tests**: `tests/e2e/` -- exercise the **CLI** only (the launcher, which since #721 runs `run_cli` in-process through the binding rather than spawning a bundled binary) with **nothing mocked**. That change makes the "nothing mocked" claim *more* faithful, not less: the suites now run the exact path a user runs. **No mocks, no fakes, no monkeypatching. NOT run in CI** -- CI verifies only the per-package *attestation* that they ran (see *E2E Attestation*).
- **Distcheck tests**: **not** an SDK-package tier -- the *functional* publishability flows (build, pack, install, and run the published artifact) live in the `internals/distcheck` package (#520), which itself follows the three-tier layout. **Run in CI** via that package's `dirsql-distcheck python` / `dirsql-distcheck node` entry points (the `distcheck` jobs in `dirsql-python-ci.yml` / `dirsql-typescript-ci.yml`). Distinct from the **`packaging` gate** (testing-conventions, run from each language's CI workflow), which only asserts no test files *ship* in the built artifact and never installs or runs it.

### Enforcing Colocation (testing-conventions)

The colocation rule is a blocking CI gate ([`testing-conventions`](https://github.com/thekevinscott/testing-conventions), wired in the per-domain CI workflows + `testing-conventions.toml`): every source file needs a colocated unit test (Rust: inline `#[cfg(test)]`), a PR-only co-change check flags modified sources whose test didn't change, and `unit lint` enforces **isolation** -- a unit test must mock every collaborator (Python `patch(...)`, TS `vi.mock`, Rust trait double or relocate the effectful test), never weaken the test. The gates also cover the two binding crates and the repo-tooling `internals/checks` / `internals/distcheck` uv packages. **The exemption count is zero and stays there**: barrels get a colocated surface test, logic gets extracted, dead shells get deleted -- an exemption is never the escape hatch. Full wiring, per-package gate map, and history: `agents/reference/testing-gates.md`.

**Run every gate CI declares before pushing** with `just preflight` (#781), which *derives* the (source, gate) pairs from the CI workflows instead of restating them -- 40 pairs across 8 scan roots, versus the 6 across 3 the four hand-written recipes it replaced covered. It reads every `.github/workflows/*.yml` and folds together each job that calls the reusable workflow, so a lane added in a new per-domain file is a pair it runs. It also encodes the invocations that differ from the naive one (python suites via the package venv, typescript via `npx`, per-gate option support), each of which used to pass locally while failing in CI. `--dry-run` prints the matrix; `--gate <name>` and `--conventions <workflow>` narrow it (both repeatable); `packaging` reports SKIP since it needs a built artifact (`just test-packaging`).

### Mutation (testing-conventions)

The `unit mutation` gate mutates PR-changed source lines and fails on any mutant no unit test kills (engines: cosmic-ray / Stryker / cargo-mutants, all self-provisioned; PR-only, diff-scoped via `--base`). All three SDKs run it inside their own CI workflow. Fix a survivor with a **new assertion**, never by weakening a test; only a genuinely equivalent mutant gets a reason-required `mutation` exemption (none today). Local run commands and per-language details: `agents/reference/testing-gates.md`.

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

CI never runs the e2e suites; instead the `e2e-verify` gate in each package's CI workflow checks, per package, a committed attestation (`packages/python/e2e-attestation.json`, `packages/ts/e2e-attestation.json`) recording that the suite ran against the package's current code. **Regenerate the attestation for each package you changed, as the last commit touching that package before you push**: `just e2e-attest-python` / `just e2e-attest-ts` (needs `pip install testing-conventions`). Multi-package PRs: attest each package right after finishing *its* changes -- attesting both at the very end leaves the second one stale. **Any `packages/rust/src` change (including `cli/`) stales BOTH bindings' attestations** (CI-enforced via `[e2e].extra_scope`), so re-attest both after core changes. **A binding-crate change stales only its own package** -- `packages/python/src` (PyO3) the python attestation, `packages/ts/napi` the ts one (#933); that per-package scope is why each SDK package carries its own `testing-conventions.toml`. Full mechanics and debugging: `agents/reference/e2e-attestation.md`.

### Docs as Reference

**Docs are the canonical description of intended behavior** -- the human source of truth for what the product does. They carry **no** test obligation: there is no rule that every documented feature must have a test, that every test must trace back to a doc, or that tests update whenever docs change. Docs themselves are not tested.

Product *behavior* stays covered by the normal unit / integration / binding / e2e tiers -- that is unchanged. When adding a feature, the PR still includes docs (the human description) and whatever behavior tests the change warrants; the two are simply no longer coupled by a gate.

### Changelog and Migrations

**Every PR touching non-test source under `packages/<pkg>/` or `plugins/<pkg>/` must add a changelog fragment** at `<root>/<pkg>/changelog.d/YYYY-MM-DD-<slug>.md` (UTC merge date; body leads with a bold Keep-a-Changelog category, e.g. `**Fixed**`), one per changed package. **A package `README.md` counts as source here** -- it ships in all three published artifacts, so a README edit runs `release-ci` + `changelog-check` and needs a fragment, unlike the root prose files. Enforced by the `changelog-gate` check in CI. Escape hatch for truly no-observable-change PRs: a `skip-changelog: <reason>` line in any commit message. Fragments are permanent and append-only; the root `CHANGELOG.md` / `MIGRATIONS.md` are **frozen** pointer stubs -- never write into them; stray entries get relocated into the owning package's fragment dir, never folded into the frozen files.

**A migration fragment** (`<root>/<pkg>/migrations.d/YYYY-MM-DD-<slug>.md`) is additionally required when a PR breaks a public API, removes a deprecated symbol, or changes runtime behavior without an API change. It needs all five subsections (Summary / Required changes / Deprecations removed / Behavior changes without code changes / Verification); write `_None._` under any that don't apply. Template at the bottom of the frozen root `MIGRATIONS.md`. Purely additive changes and behavior-preserving fixes need no migration entry.

Full mechanics (scope/exemptions, fragment format details, relocation rules): `agents/reference/changelog-migrations.md`.

**PR body requirement:** PRs that touch SDK code must contain the following block (checkboxes filled in):

```markdown
## Changelog / Migrations

- [ ] Changelog fragment added under `<root>/<pkg>/changelog.d/` for each changed package (or: `skip-changelog` trailer on a commit with reason)
- [ ] Migration fragment added under `<root>/<pkg>/migrations.d/` (or: not required -- additive/bugfix only)
```

Orchestrators must block merges of SDK-touching PRs that miss either file when required.

### Cross-SDK Parity (PARITY.md)

`PARITY.md` is a **living document** that tracks API-surface parity across the Python, Rust, and TypeScript SDKs. It must stay current.

On every PR that touches any SDK's public API (`packages/python/dirsql/`, `packages/rust/src/`, or `packages/ts/src/`), the author must:

1. Update `PARITY.md` to reflect the new/changed/removed API surface.
2. Call out in the PR body whether the change is **introducing parity drift** (one SDK gets something the others don't yet) or **restoring parity** (bringing a lagging SDK in line). Drift is allowed but must be intentional and tracked.
3. If drift is introduced, open a follow-up GitHub issue for each lagging SDK so the gap is visible.

Orchestrators must block merges of SDK-touching PRs that don't update `PARITY.md`.

### Benchmarks

Run `cargo bench -p dirsql` after significant changes to the Rust codebase. Not in CI -- local only. Covers: SQLite operations, directory scanning, row diffing, glob matching. Use to catch performance regressions before merging.

## Git and GitHub Workflows

### Releases: Merging to Main Publishes

**Merging to `main` is the release trigger — there is no separate release step.** dirsql publishes via [putitoutthere](https://github.com/thekevinscott/putitoutthere) (`putitoutthere.toml` at the repo root; `release.yml` calls its `@v0` reusable workflow on every push to `main`). Each merge whose changed files match a package's release `globs` publishes that package to its registry immediately, with no version bump, tag, or human sign-off in between — and `depends_on` cascades: a `packages/rust/**` change republishes the PyPI and npm packages too. A merge matching no package's globs (root config, docs, CI) publishes nothing. This is why agents shepherd PRs to green and **stop**: merging is publishing, and only the maintainer merges.

### PR Sizing and Issues

- **Every PR is M or smaller.** A change larger than M is broken into a sequence of smaller PRs, each independently reviewable and mergeable. Size by review surface, not raw line count -- a mechanical rename spanning many files can be M, while a subtle core change of far fewer lines may not be.
- **Stack that sequence; do not serialize it.** When an epic's slices depend on each other, open each PR with its `base` set to the branch below it rather than waiting for that branch to merge. Reviewers see each layer's own diff, and the slices land in order without anyone idling. [Stacked pull requests](https://github.blog/changelog/2026-07-30-stacked-pull-requests-are-now-in-public-preview/) are GitHub-native, but the mechanism is just the base branch, so it works from `mcp__github__create_pull_request` (set `base`) without any preview-only tooling -- relevant because `gh` (and therefore the `gh-stack` skill) is unavailable in the hosted sandbox. Three things are already true and need no special handling: **GitHub retargets a child to `main` automatically** when its parent merges; **branch protection and required checks apply per layer**, unchanged; and **diff-scoped gates measure the layer, not its ancestors** -- every CI workflow passes `base: ${{ github.event.pull_request.base.sha }}`, so `unit mutation`, changed-lines coverage and `changelog-gate` all compute against the stack parent (verified on #703 while stacked: `BASE: 0241d80…`, the tip of `claude/699-read-pdf`). Only slices with a real dependency belong in one stack; independent slices are ordinary PRs off `main`.
- **Never request the maintainer's review on a PR.** Do not add reviewers by any mechanism -- `gh pr create --reviewer`, `gh pr edit --add-reviewer`, the GitHub API, or MCP tools. Each request fires a notification; the workflow is already shepherd-to-green and report status, with the maintainer deciding when to look and merge. Opening the PR is the signal; a review request adds nothing but noise.
- **Every PR is accompanied by a GitHub issue it auto-closes.** Put a closing keyword (`Fixes #<n>` / `Closes #<n>`) in the PR body so the issue closes on merge; file the issue first if one does not exist yet.
- **Every PR gets a unique branch name -- NEVER reuse a branch name across PRs, even for follow-up work on the same issue/epic.** After a PR merges (or when starting a new PR), branch a fresh, distinctly-named branch; do **not** re-create the just-merged branch name and stack the next change on it. Reuse hides real code divergence: an unrelated PR that merged to `main` in the meantime can textually auto-merge with your branch into a **compile error** (e.g. `main` adds a call to a helper your branch deleted). A unique branch per PR forces an honest three-way merge against current `main`. It also keeps each e2e attestation receipt on its own path (`packages/<pkg>/e2e-attestations/<branch>.json`), so a re-used name does not have two PRs writing the same file. (Learned the hard way in epic #601: #602 and #603 both used `claude/tackle-601-kxeykd`.)

### Merge Conflict Resolution

**Merge conflicts on in-flight PRs are the highest priority.** When asked to resolve a merge conflict:

1. Immediately stop other work and focus on the conflict.
2. Pull the latest main/base branch.
3. Resolve the conflict carefully, preserving both intended changes where possible.
4. Commit and push the resolution.
5. Do not proceed with other tasks until the merge conflict is fully resolved and the branch is clean.

### PR Monitoring

Merge gating is via **pr-monitor** (`.github/workflows/pr-monitor.yml`, the **`CI Gate`** check — [thekevinscott/pr-monitor](https://github.com/thekevinscott/pr-monitor) at the rolling `@v1`). It is an *aggregator*, not a test, and it does not guess at the check set: [willfire](https://github.com/thekevinscott/willfire) evaluates the repo's workflow files against the PR's base branch, changed files and head commit to get the exact set of workflow runs GitHub will create, and the gate is a set comparison against the runs on the head — yellow while a predicted run is missing or unfinished, red when one concludes anything but `success` / `skipped` / `neutral` / `stale`, and red when a run appears that nothing predicted. Branch protection requires only `CI Gate`, so adding/renaming/removing jobs or workflows needs no branch-protection or gate-config coordination — don't flag that concern.

**Zero predicted runs is a pass, and that is a derived verdict rather than a blind spot.** Post-#834 a diff can legitimately trigger nothing (docs, `agents/`, `notes/`); the gate reads the same workflow files GitHub does, so an empty required set means the diff really does trigger no checks. Two corollaries: a path-filtered workflow still in flight **does** hold the gate (it was predicted, so it is required — the opposite of the check-counting era), and a workflow that should have dispatched but didn't hangs the gate instead of passing silently. #862 would have closed the residual CI-config-only gap with a `ci-paths` check; it is closed `not_planned` because prediction subsumes most of it.

**There is nothing to tune** — `github-token` is the whole input surface, and `pre-sleep` / `minimum-checks` / `timeout` / `job-name` / `excluded-jobs` were removed upstream when prediction landed (GitHub ignores them with a warning, so a config still passing them is doing nothing). `timeout-minutes: 20` on the job is the backstop, not a gate setting. When the gate is red or stuck, **read its log first — it prints `Required: [...]`, the prediction**: `Non-passing runs` is an ordinary CI failure, an *unexpected* run is a willfire modelling gap, and a timeout is either over-prediction or a hung job (diff `Required:` against the runs that exist to tell which). Full semantics, the configuration constraints, per-shape debugging, and why check-run counting could not work: `agents/reference/pr-monitor.md`.

**Gate changes are testable on the PR that makes them.** The gate is an ordinary workflow, so a PR editing `pr-monitor.yml` is gated by its own edited copy — unlike Mergify, which read config from `main` only and forced land-and-canary cycles to test anything. **Flaky jobs are manual**: re-run by hand to unblock, then fix or quarantine.

**Auto-merge is GitHub-native:** enable auto-merge on the PR (UI or `mcp__github__enable_pr_auto_merge`) and it merges once branch protection is satisfied — `CI Gate` green plus an approving review. The old Mergify `auto-merge` label does nothing now.

When monitoring PRs to get them across the finish line (shepherding to green):

1. **Watch for merge conflicts** in addition to CI status. If a PR becomes unmergeable due to conflicts, immediately flag and work to resolve.
2. **Monitor for GPG signing failures** if the repo requires signed commits. Re-sign or re-commit as needed to pass signature checks.
3. Check CI logs for any signing-related errors and address them before merge.
4. Keep the user informed of blockers and resolution status.

### Coverage Floor

All three SDKs enforce unit-coverage floors via testing-conventions `unit coverage`; floors live in `testing-conventions.toml`. Python/TS run inside `dirsql-python-ci.yml` / `dirsql-typescript-ci.yml` at 100% (plus a PR-only changed-lines check); the Rust core is bespoke in `dirsql-rust-ci.yml` (nightly `cargo llvm-cov --lib --features cli --branch`; floors: lines 94 / regions 93 / functions 91 / branch 75, pinned near actuals -- if adding tests lowers `branch`, nudge the floor down rather than chasing it). **Floors apply to unit tests only** -- integration/binding/e2e never pad the number; every covered source file needs in-process unit tests (inject and mock the binding layer rather than excluding a file; only line-scoped, reason-required coverage exemptions exist, and there are none today). Full rationale, Rust branch-floor caveats, and exemption rules: `agents/reference/testing-gates.md`.
