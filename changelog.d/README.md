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

The dispatch-triggered "Release Notes" workflow
(`.github/workflows/release-notes.yml`) assembles the fragments into a dated
`CHANGELOG.md` section (towncrier, config in `towncrier.changelog.toml`) and
deletes them, via an assemble PR. Preview pending entries with:

    uvx towncrier@25.8.0 build --config towncrier.changelog.toml --draft --version next

See AGENTS.md, section "Changelog and Migrations". This README is not a
fragment and never satisfies the changelog gate.
