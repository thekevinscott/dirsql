# changelog.d — TypeScript SDK

One changelog fragment per change, so PRs never conflict on a shared file.
Each PR that touches this package's public-facing source adds one file here:

    changelog.d/YYYY-MM-DD-<slug>.md

- `YYYY-MM-DD` — the UTC merge date (newest sorts last).
- `<slug>` — a short kebab-case description of the change.
- **Body** — the entry as it should read, leading with its Keep a Changelog
  category: `**Added**` / `**Changed**` / `**Deprecated**` / `**Removed**` /
  `**Fixed**` / `**Security**`.

Fragments are **permanent and append-only** — nothing is ever assembled back
into a single `CHANGELOG.md`, and the release↔entry mapping comes from
`git log --tags`. This README is not a fragment and never satisfies the
changelog gate. See AGENTS.md, "Changelog and Migrations".
