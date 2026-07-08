# Handoff: adopt the `packaging` gate for Rust in `conventions.yml` (closes #413)

**Audience:** implementing agent. Follow this document exactly; where it conflicts
with your instincts, this document wins. Read `AGENTS.md` first — its CI rules
(especially "Reusable-workflow gates: adoption & debugging") apply throughout.

## Context (verified 2026-07-08, do not re-derive)

- dirsql runs the [testing-conventions](https://github.com/thekevinscott/testing-conventions)
  reusable workflow per package in `.github/workflows/conventions.yml`. Python and
  TypeScript already run the `packaging` gate there, zero-config (issue #413, PR #448).
- Rust could not adopt it: `packages/rust` is a **Cargo workspace member**, and
  `cargo package` writes to the **workspace root's** `target/package/`, never the
  member's own, so the gate's zero-config auto-build could not find the `.crate`.
  The bespoke `.github/workflows/packaging.yml` (rust-only) enforces the rule instead.
- **That upstream gap is now fixed**: testing-conventions issue #360 was resolved by
  PR #362 (merged 2026-07-07). The fix adds `is_workspace_member()` to `detect.py`
  (walks up from the package root looking for an ancestor `Cargo.toml` with a
  `[workspace]` table) and, for members, appends `--target-dir target` to the derived
  `cargo package` command so the artifact lands under the member's own
  `target/package/` where the gate scans. **No consumer configuration is needed.**
- Verified live: `git ls-remote https://github.com/thekevinscott/testing-conventions v0 main`
  shows `v0` == `main` tip (`ca7345e4fc7313115d7b6dcf34dba969bbea0ba5`), which
  postdates the #362 merge. The fix is what `@v0` callers run today.

## The task, in two strictly ordered stages

Per AGENTS.md: **never retire a bespoke gate ahead of a green proof.** Stage 1 is a
probe PR that adds the gate while keeping the bespoke workflow. Stage 2 is a separate
follow-up PR that retires the bespoke workflow, opened only after Stage 1's gate is
observed green in CI. Do not combine them.

### Stage 1 — probe PR: add `"packaging"` to the rust caller

1. In `.github/workflows/conventions.yml`, `rust:` job, change:

   ```yaml
   gates: '["colocated-test", "unit-lint", "integration-lint", "mutation"]'
   ```

   to:

   ```yaml
   gates: '["colocated-test", "unit-lint", "integration-lint", "mutation", "packaging"]'
   ```

   The gate name is exactly `packaging` (lowercase). Do **not** add any other input
   (`rust_toolchain`, `build_command`, etc.) — the gate auto-provisions cargo and the
   build itself, and unknown/removed inputs **startup-fail the entire workflow** for
   every PR and `main` (see AGENTS.md).

2. Update the now-stale comments in the same file — both places:
   - The header comment block that says `packaging` "is PROBED here for
     python/typescript only … Rust can't adopt it yet" — rewrite to say rust now
     probes it too, upstream #360/#362 having fixed workspace-member support, and
     that `packaging.yml` stays until this probe is confirmed green.
   - The comment inside the `rust:` job beginning `# No "packaging" here:` — replace
     with a short note that workspace-member support landed upstream (#360, fixed by
     PR #362: membership detected from ancestor `Cargo.toml` `[workspace]`, build
     redirected via `--target-dir target`).

3. **Do not touch** `.github/workflows/packaging.yml` in this PR. It remains the
   enforcement mechanism until the probe is proven.

4. No other files change. `testing-conventions.toml` needs nothing — the fix is
   config-free. This PR touches no SDK source, so: no `CHANGELOG.md` entry needed
   (the changelog gate only fires on SDK paths), no `MIGRATIONS.md`, no `PARITY.md`,
   no e2e attestation refresh.

5. Branch/commit/PR mechanics: create a branch off the latest `main` (never commit to
   `main`), one PR for this stage. PR body must include the `## E2E Verification`
   section from AGENTS.md with the single line `N/A - docs/lint/typo only` (this is
   CI-workflow-config only).

6. **Observe CI on the probe PR.** Expected: the `rust` caller grows a
   `Packaging (no test files in the built artifact)` job (name may differ slightly;
   match on "Packaging") and it must be **green — not skipped**. A skipped packaging
   job means the gate didn't activate; treat that as a failure of the probe and
   diagnose (see below). The bespoke `Packaging / No test files in built artifacts`
   check from `packaging.yml` will also run (this PR touches neither
   `packages/rust/**` nor `packaging.yml`, so it may not trigger — that's fine; it
   is not the probe's subject).

### Stage 1 failure playbook

- **`startup_failure`, 0 jobs:** you passed an invalid input or malformed `gates`
  JSON. Do NOT hypothesize: WebFetch the run's `html_url` and read the annotation
  (`Invalid workflow file: conventions.yml#L<n> — Invalid input, <name> is not
  defined…`). Fix exactly what it names. There are no job logs in this state.
- **Packaging job red, "no artifact found" / can't locate `target/package`:** the
  upstream fix isn't behaving as documented for our layout. Capture the full job log,
  comment your findings on dirsql #413 (include the run URL and the derived command
  from the log), and stop — do not work around it with `[rust].build_command` or
  `packaging_artifact`, and do not retire anything. (`@v0` can roll mid-run; if you
  suspect skew, re-run once, then pin-read the workflow at the sha from
  `git ls-remote … v0` before concluding.)
- **Packaging job red, actual test files found in the crate:** that is a real
  hygiene failure — the crate ships test files. Fix the crate (exclude the files via
  `Cargo.toml` `exclude`/`include`), don't waive the gate.
- **Every rust gate suddenly red including previously-green ones:** you broke
  `testing-conventions.toml` or the caller — an unknown config key fails EVERY gate.
  Revert your config change; the probe requires no config.

### Stage 2 — follow-up PR: retire the bespoke workflow (only after Stage 1 is merged and its gate observed green)

1. Delete `.github/workflows/packaging.yml` entirely.
2. In `conventions.yml`, update the header comment again: `packaging` is no longer a
   probe — all three SDKs run it whole-hog; the bespoke `packaging.yml` is deleted.
3. In `AGENTS.md`, find the "Smoke tests" bullet under "Test Locations" — it says the
   packaging gate is "run via `conventions.yml` for python/typescript and
   `.github/workflows/packaging.yml` for the rust crate". Update it: the gate runs
   via `conventions.yml` for all three languages.
4. Same PR-body rules as Stage 1 (`N/A - docs/lint/typo only`).
5. After merge: comment on dirsql issue **#413** summarizing both stages (probe PR,
   green run URL, retirement PR) and close it. Also comment on **#240** that the
   packaging row is now whole-hog for all three SDKs (leave #240 open — #419 remains).

## Acceptance criteria

- `conventions.yml` `rust` job runs `packaging` and it is green on PRs.
- `.github/workflows/packaging.yml` no longer exists.
- No new entries in `testing-conventions.toml` (exemption count stays 1, pending #419).
- #413 closed with links to both PRs and a green run.
