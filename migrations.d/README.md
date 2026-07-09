# migrations.d

Migration-entry fragments. Instead of editing `MIGRATIONS.md` (which
merge-conflicts with every other in-flight PR), a PR that breaks a public
API, removes a deprecated symbol, or changes runtime behavior adds one new
file here:

    migrations.d/<branch-slug>.md

- `<branch-slug>`: the PR's branch name, lowercased and sanitized (same slug
  format as testing-conventions' branch-keyed e2e receipts).
- Content: one complete migration entry following the template at the bottom
  of `MIGRATIONS.md` — a `### <title>` heading plus all five required
  subsections (Summary / Required changes / Deprecations removed / Behavior
  changes without code changes / Verification), with `_None._` under any
  subsection that does not apply.

At release time the fragments are assembled into `MIGRATIONS.md` and deleted.
See AGENTS.md, section "Changelog and Migrations". This README is not a
fragment.
