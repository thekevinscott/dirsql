# migrations.d — Python SDK

One migration fragment per change, twinned with the changelog fragment, so
PRs never conflict on a shared file. Each PR that touches this package's
public-facing source adds one file here, whether or not anything breaks:

    migrations.d/YYYY-MM-DD-<slug>.md

- `YYYY-MM-DD` — the UTC merge date (newest sorts last).
- `<slug>` — a short kebab-case description.
- **Body** — one complete migration entry following the five-subsection
  template (Summary / Required changes / Deprecations removed / Behavior
  changes without code changes / Verification); keep every heading, writing
  `_None._` where a subsection does not apply -- every one but Summary when
  nothing breaks.

Fragments are **permanent and append-only** — nothing is assembled back into a
single `MIGRATIONS.md`. This README is not a fragment. See
`agents/reference/changelog-migrations.md`.
