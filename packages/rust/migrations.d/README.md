# migrations.d — Rust SDK

One migration fragment per breaking change, so PRs never conflict on a shared
file. A PR that breaks this package's public API (signature, name, return
type, config key, CLI flag) or changes its runtime behavior without changing
the API adds one file here:

    migrations.d/YYYY-MM-DD-<slug>.md

- `YYYY-MM-DD` — the UTC merge date (newest sorts last).
- `<slug>` — a short kebab-case description.
- **Body** — one complete migration entry following the five-subsection
  template (Summary / Required changes / Deprecations removed / Behavior
  changes without code changes / Verification); keep every heading, writing
  `_None._` where a subsection does not apply.

Fragments are **permanent and append-only** — nothing is assembled back into a
single `MIGRATIONS.md`. This README is not a fragment. See AGENTS.md,
"Changelog and Migrations".
