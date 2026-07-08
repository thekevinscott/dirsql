# Handoff: delete the last testing-conventions exemption (closes #419)

**Audience:** implementing agent. Follow this document exactly. Read `AGENTS.md`
first — especially "Enforcing Colocation (testing-conventions)".

## Context (verified 2026-07-08, do not re-derive)

dirsql's `testing-conventions.toml` carries exactly **one** exemption:

```toml
[[python.exempt]]
path = "__init___test.py"
rules = ["unmocked-collaborator"]
```

It existed because the python `unit lint` isolation rule flagged the barrel test
(`packages/python/dirsql/__init___test.py`, which does
`from . import DirSQL, RowEvent, Table, __all__, __version__`) as importing five
unmocked collaborators — even `__all__`, defined in the SUT itself — while the
identical TS pattern (`index.test.ts` importing `./index.js`) passed. dirsql issue
**#419** tracks deleting it.

**The upstream gap is now fixed:**

- Filed as testing-conventions **#382** (python `unmocked-collaborator` flags a
  barrel test's own SUT imports; TS equivalent passes).
- Fixed by **PR #384**: a bare package-relative import (`from . import <names>`,
  `module: None`) in a barrel test resolves to the SUT and is permitted; a
  sibling-reaching import (`from .core import Thing`) is still flagged, so the rule
  is not weakened.
- **Verified live against this repo** on the released CLI (`testing-conventions
  0.0.66`): with the exemption block deleted,
  `npx -y testing-conventions@latest unit lint --language python --config
  testing-conventions.toml packages/python/dirsql` exits **0**.

No upstream work, no filing, no waiting. This is now a small dirsql-only PR.

## The task (one PR, branch off latest `main`, never commit to `main`)

1. In `testing-conventions.toml`:
   - Delete the entire `[[python.exempt]]` block (`path` / `rules` / `reason`) AND
     the explanatory comment block immediately above it (the paragraph beginning
     `# The public barrel `__init__.py` is verified by its colocated test`). Keep
     the `# --- Python (scanned: packages/python/dirsql) ---` section header.
   - Rewrite the header comment paragraph that begins `# There is exactly ONE
     exemption below…` to state the exemption count is **zero**: every barrel is
     verified by a colocated test with no waivers; the former python isolation
     parity gap was testing-conventions#382, fixed by testing-conventions PR #384.
2. In `AGENTS.md`, "Exemptions" paragraph (under "Enforcing Colocation"): it
   currently says "Exactly **one** exemption remains…" and explains the parity gap
   at length, ending with "when it lands, this last entry is deleted and the
   exemption count is zero." Rewrite to: the exemption count is **zero**; briefly
   note (one or two sentences) that the last entry — the python barrel-test
   isolation waiver — was removed once testing-conventions#382 / PR #384 brought
   the python rule to parity with TS. Keep the surrounding guidance ("Adding a
   *new* untested source file fails the gate — an exemption is never the escape
   hatch").
3. Do NOT touch `__init___test.py`, any `packages/` source, or anything else.

## Verification (run before pushing; separate commands, never chained)

```bash
npx -y testing-conventions@latest unit lint --language python --config testing-conventions.toml packages/python/dirsql
```

Expect exit 0. Also run the TS control (expect exit 0, unchanged):

```bash
npx -y testing-conventions@latest unit lint --language typescript --config testing-conventions.toml packages/ts/src
```

Then let CI confirm via the `python-unit` job in `conventions.yml` (its `unit-lint`
gate). If it is red with `unmocked-collaborator` on `__init___test.py`, the CI
runner resolved an older CLI than 0.0.66 — report on #419 with the run URL; do not
re-add the exemption without recording why.

## PR / bookkeeping

- PR body: include the `## E2E Verification` section from AGENTS.md with the single
  line `N/A - docs/lint/typo only` (config + docs only; no SDK source). No
  `CHANGELOG.md` / `MIGRATIONS.md` / `PARITY.md` updates, no attestation refresh.
- After merge: comment on and **close #419** (cite testing-conventions#382 /
  PR #384, CLI 0.0.66, and the green run). Comment on **#240** that the exemption
  count is now **zero** — one of its two remaining checkboxes. #240 itself stays
  open until rust packaging (#413) also lands.

## Do NOT

- Do not weaken or delete `__init___test.py`.
- Do not add any new exemption.
- Do not fold this into the rust-packaging PR — separate concerns, separate PRs.
