# changelog.d

Changelog fragments. Instead of editing `CHANGELOG.md` (which merge-conflicts
with every other in-flight PR), each PR adds one new file here:

    changelog.d/<branch-slug>.<category>.md

- `<branch-slug>`: the PR's branch name, lowercased and sanitized (same slug
  format as testing-conventions' branch-keyed e2e receipts).
- `<category>`: one of `added`, `changed`, `deprecated`, `removed`, `fixed`,
  `security` (Keep a Changelog).
- Content: the entry body exactly as it would appear under `## [Unreleased]`
  in `CHANGELOG.md` — typically one bold-led bullet.

At release time the fragments are assembled into `CHANGELOG.md` and deleted.
See AGENTS.md, section "Changelog and Migrations". This README is not a
fragment and never satisfies the changelog gate.
