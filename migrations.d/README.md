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

The dispatch-triggered "Release Notes" workflow
(`.github/workflows/release-notes.yml`) assembles the fragments into a dated
`MIGRATIONS.md` section (towncrier, config in `towncrier.migrations.toml`)
and deletes them, via an assemble PR. Preview pending entries with:

    uvx towncrier@25.8.0 build --config towncrier.migrations.toml --draft --version next

See AGENTS.md, section "Changelog and Migrations". This README is not a
fragment.
