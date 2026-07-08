# Handoff: file the python/ts isolation-parity gap upstream (unblocks #419)

**Audience:** implementing agent. Follow this document exactly. Read `AGENTS.md`
first — especially "Enforcing Colocation (testing-conventions)" and the
"Reusable-workflow gates" rules.

## Context (verified 2026-07-08)

dirsql's `testing-conventions.toml` carries exactly **one** exemption:

```toml
[[python.exempt]]
path = "__init___test.py"
rules = ["unmocked-collaborator"]
```

dirsql issue **#419** tracks driving that count to zero and says it is "blocked on
the upstream python/ts isolation-parity fix (filed)". **That upstream issue appears
to have never been filed** — searches of
`thekevinscott/testing-conventions` issues (open and closed, terms: parity,
isolation, barrel, unmocked, `__init__`, unit lint) found nothing matching, as of
2026-07-08. dirsql's own comments in `testing-conventions.toml` (lines ~24 and ~77)
and `AGENTS.md` also claim "filed upstream". Your job: verify the gap still exists on
the current CLI, file the upstream issue (body below), and fix dirsql's stale
"filed upstream" references to link it.

## The gap, precisely

Both SDK barrels are verified by colocated tests, per the repo's zero-exemption
policy (a barrel is *tested*, never waived):

- **Python:** `packages/python/dirsql/__init__.py` ↔ `__init___test.py`, which does
  `from . import DirSQL, RowEvent, Table, __all__, __version__` and asserts
  `set(__all__) == {"DirSQL", "Table", "RowEvent", "__version__"}` plus that each
  name resolves to a real value.
- **TypeScript:** `packages/ts/src/index.ts` ↔ `index.test.ts`, which does
  `import * as api from "./index.js"` and asserts the exact export set.

These are the *same pattern*: the test imports **its own SUT** (the colocated barrel)
and asserts the re-exported surface. The TS `unit lint` isolation rule passes it. The
python rule flags every imported name as an `unmocked-collaborator` — **including
`__all__`, which is defined in `__init__.py` itself**, i.e. it flags the SUT as a
collaborator of its own test. Last verified on CLI v0.0.53: removing the exemption
produced **5 violations** (one per imported name). Mocking the names would make the
test assert nothing, so dirsql waives the isolation rule *on the test* (keeping the
barrel genuinely verified) — the only exemption left in the repo.

## Step 1 — re-verify the gap on the CURRENT CLI release

The gap may have been fixed since v0.0.53. From a clean dirsql checkout at `main`:

1. Delete the entire `[[python.exempt]]` block (the comment block above it can stay
   for now) from `testing-conventions.toml`.
2. Run both of these (do not chain them; run as separate commands):

   ```bash
   npx -y testing-conventions@latest unit lint --language python --config testing-conventions.toml packages/python/dirsql
   ```

   ```bash
   npx -y testing-conventions@latest unit lint --language typescript --config testing-conventions.toml packages/ts/src
   ```

3. Interpret:
   - **python exit 1 with `unmocked-collaborator` violations on `__init___test.py`,
     typescript exit 0** → gap confirmed. Restore the exemption block exactly as it
     was (`git checkout -- testing-conventions.toml`), record the CLI version
     (`npx -y testing-conventions@latest --version`) and the verbatim violation
     output, and proceed to Step 2.
   - **python exit 0** → the gap is already fixed upstream. Do NOT file anything.
     Instead skip to Step 3 variant B (delete the exemption for real — that
     completes #419).
   - **typescript exit 1** or unrelated python failures → something else is wrong
     (config typo, tool regression). Stop and report on dirsql #419; file nothing.

## Step 2 — file the upstream issue

File on `thekevinscott/testing-conventions`. If your session lacks write scope for
that repo, request it be added (`add_repo`) or, failing that, hand the text below to
the maintainer verbatim and note that on dirsql #419. Do not silently skip filing.

Before filing, replace `<CLI_VERSION>` and `<PASTE OUTPUT>` with the real values
from Step 1.

**Title:**

```
unit lint (python): isolation rule flags a barrel test importing its own SUT (`from . import …` in `__init___test.py`) — the identical TS pattern passes
```

**Body:**

````markdown
## Summary

The python `unit lint` isolation rule (`unmocked-collaborator`) and the TypeScript
rule disagree on the same pattern: a colocated barrel test importing the barrel it
verifies.

A re-export barrel can only be verified by importing its public surface. The TS rule
accepts `index.test.ts` doing `import * as api from "./index.js"`. The python rule
flags `__init___test.py` doing `from . import DirSQL, RowEvent, Table, __all__,
__version__` — one `unmocked-collaborator` violation per imported name, **including
`__all__`, which is defined in `__init__.py` itself**. That is, the rule treats the
SUT as an unmocked collaborator of its own colocated test.

Mocking these imports would make the barrel test assert nothing, so the only current
consumer options are (a) leave the barrel untested (violates colocated-test) or
(b) carry a permanent `unmocked-collaborator` exemption on the test. dirsql carries
exactly one exemption today and it is this one (thekevinscott/dirsql#419 tracks
deleting it the moment this is fixed).

## Reproduction

On testing-conventions <CLI_VERSION>, against https://github.com/thekevinscott/dirsql
at `main`, with the `[[python.exempt]]` block for `__init___test.py` removed from
`testing-conventions.toml`:

```bash
npx -y testing-conventions@<CLI_VERSION> unit lint --language python --config testing-conventions.toml packages/python/dirsql
# exit 1:
<PASTE OUTPUT — the 5 unmocked-collaborator violations on __init___test.py>
```

Control — the identical TS pattern passes:

```bash
npx -y testing-conventions@<CLI_VERSION> unit lint --language typescript --config testing-conventions.toml packages/ts/src
# exit 0 (index.test.ts imports ./index.js and is not flagged)
```

The two tests under discussion:

- `packages/python/dirsql/__init___test.py` — `from . import DirSQL, RowEvent,
  Table, __all__, __version__`; asserts the exact `__all__` set and that each name
  resolves.
- `packages/ts/src/index.test.ts` — `import * as api from "./index.js"`; asserts the
  exact export set.

## Proposed fix

Bring python to parity with TS: an import that **resolves to the colocated source
module under test** is the SUT, not a collaborator. For a test named
`__init___test.py`, the SUT is the package's `__init__.py`, so `from . import …`
(and `from <pkg> import …` resolving to the same `__init__.py`) must be exempt from
`unmocked-collaborator` — exactly as `index.test.ts`'s `./index.js` import already
is. Names re-exported *through* the SUT are the SUT's public surface being verified,
not out-of-unit reaches; `__all__` in particular is defined in the SUT.

Red-path guard so the rule isn't weakened: a barrel test that imports a **sibling
module directly** (e.g. `from .core import DirSQL` inside `__init___test.py`,
bypassing the barrel) must still be flagged — the exemption is only for imports
resolving to the SUT module itself.

## Acceptance

- The python reproduction above exits 0 with no exemption.
- The sibling-direct-import red-path case is flagged.
- dirsql deletes its last exemption (thekevinscott/dirsql#419) and its
  `testing-conventions.toml` exemption count reaches zero.
````

## Step 3 — update dirsql (separate small PR on a branch off `main`)

**Variant A (gap confirmed, issue filed — the expected path):**

1. Replace every stale "filed upstream" claim with a link to the real issue number
   you filed. Exact locations:
   - `testing-conventions.toml`: the header comment ("a parity gap filed upstream")
     and the comment block above `[[python.exempt]]` plus the `reason` string
     ("(filed upstream)") — cite `thekevinscott/testing-conventions#<N>`.
   - `AGENTS.md`, "Exemptions" paragraph: "This is a testing-conventions python/ts
     parity gap filed upstream" — add the issue link.
2. Comment on dirsql **#419** with the upstream issue link and the re-verification
   result (CLI version, violation count). #419 stays open — it closes only when the
   upstream fix lands and the exemption is deleted.
3. PR body: `## E2E Verification` section with `N/A - docs/lint/typo only`. No
   changelog/migrations/parity updates (no SDK source touched).

**Variant B (Step 1 showed python exit 0 — gap already fixed):**

1. Delete the `[[python.exempt]]` block AND its explanatory comment block from
   `testing-conventions.toml`; rewrite the header comment (lines ~16–24) to state
   the exemption count is **zero**.
2. Update `AGENTS.md`'s "Exemptions" paragraph: the last exemption is gone; the
   count is zero (trim the parity-gap explanation to past tense or remove it).
3. Re-run the python `unit lint` command from Step 1 and confirm exit 0; also let CI
   confirm (`python-unit / unit-lint` gate in `conventions.yml`).
4. Comment on and close dirsql **#419**; comment on **#240** that exemptions are at
   zero. PR body rules as in Variant A.

## Do NOT

- Do not weaken or delete `__init___test.py` to satisfy the rule.
- Do not mock the barrel's re-exports in the test (it would assert nothing).
- Do not add new exemptions of any kind.
- Do not touch `packages/` source.
