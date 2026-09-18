# Code and Repo Conventions

Naming, imports, comments, dependency hygiene, and where CI logic is allowed to live.

## Dependencies

**Never `uv pip install` (or `pnpm link`) into a package's venv during development.** Add the dependency to the manifest (`[project].dependencies`, or `[dependency-groups].dev` for a test-only one) and run `uv sync`. `uv pip install` populates the venv without declaring anything, leaving it **strictly more capable than any real install** -- so the import resolves locally in an environment no user will ever have. This is not a gate-coverage problem: every gate passes, because they all run inside the drifted venv. In #777 one undeclared `bin_shim` import sailed past 108 unit tests, 100% coverage, 27 e2e tests and a clean `ty`, then turned **seven CI jobs red** on a clean resolve (#782).

Two backstops, both in `just preflight`: `uv sync` per python root (which *removes* whatever a `uv pip install` left behind) and `dirsql-checks declared-deps <source>`, which asserts every third-party import in a tree resolves to a distribution its manifest declares. The convention is the durable fix; the gates are the safety net.

## Comments

Default to no comments. Only add one when the WHY is non-obvious -- a hidden constraint, an invariant, a workaround, something that would surprise a reader. Never write archaeology: no issue/PR references, no "added for the X flow" / "used by Y", no restating what adjacent code already says, no reviewer-directed justification. That belongs in the commit message and PR description, not the file -- it rots as the codebase evolves and the file is never re-read once merged. See #445 (trimmed exactly this style repo-wide) and CHANGELOG.md's entry for it.

## Imports

**Prefer relative imports for intra-package references.** Inside a package (Python or TypeScript), use `from .sibling import x` / `import { x } from "./sibling.js"` rather than the absolute `from packagename.sub.sibling import x` / `from "packagename/sub/sibling"`. Relative paths survive renames, signal that the import is internal, and keep cross-cutting refactors (e.g. the `_cli/` → `cli/` rename in #210) from rippling through every import statement. Absolute imports are appropriate when crossing a package boundary or referring to a public re-export.

## File Naming

**TypeScript filenames are dash-case (kebab-case).** Every `.ts` / `.mjs` / `.cjs` / `.json` file under `packages/ts/` uses kebab-case (`load-native-core.ts`, `resolve-binary.test.ts`, `dirsql.config-raises.mjs`); a single lowercase word (`index.ts`, `die.ts`, `main.ts`) is already valid kebab-case and stays. Only filenames follow this rule -- symbols *inside* a file keep their idiomatic `camelCase` / `PascalCase` names (the function in `resolve-binary.ts` is still `resolveBinary`). The convention is enforced for `src/` and `tests/` by biome's `style/useFilenamingConvention` rule (`filenameCases: ["kebab-case"]`) and applies package-wide (`tools/`, fixtures) by hand. Python (`snake_case.py`) and Rust (`snake_case.rs`) keep their own ecosystem conventions.

**Python test files use the `_test.py` suffix, not the `test_` prefix** -- a test for `foo.py` is `foo_test.py` (colocated unit tests) or `<feature>_test.py` (integration/binding/e2e tests under `tests/`), never `test_foo.py`.

## CI Workflows

**Every CI check emits actionable fix instructions on failure.** A failing check must tell the contributor exactly what to change -- the file, command, or trailer to add or edit -- not merely which rule was violated. When a check can detect a *near-miss* (a fix was attempted but malformed), it names the specific defect and how to correct it rather than falling through to a generic "not satisfied" message (e.g. the `changelog-gate` names a fragment file whose name breaks the `YYYY-MM-DD-<slug>.md` convention -- pointing at the exact file -- and its "no fragment" error prints the exact path to add; dirsql#566).

**CI logic lives in scripts, not workflow YAML.** `run:` / `github-script` steps stay trivial glue -- check out, set up a toolchain, invoke one command. Anything with iteration, `case` dispatch, conditionals, or text-munging moves to a check in the `internals/checks` uv package (a click group, one subcommand per check -- see `internals/checks/src/checks/`), invoked as a one-liner (`uv run --project internals/checks dirsql-checks <check>`), and carries **colocated unit tests** (the same testing-conventions standard as the rest of the tree -- `foo.py` ↔ `foo_test.py`). Those tests run under `internals-checks-ci.yml`'s `internals-checks` job (`unit-coverage` enforces a 100% floor; full gate list in `agents/reference/testing-gates.md`). Inline workflow logic is untestable, un-runnable locally, and silently duplicated across runners; a script is none of those.

## Scratch Files

Write scratch/temporary files to `/tmp` instead of asking permission. Use unique filenames to avoid collisions with other sessions.
Temporary scripts, including Node or shell helpers, must also be written to `/tmp` and executed from there.
