# Documentation Development

Instructions for agents working on `dirsql` documentation.

## Stack

The docs site uses [VitePress](https://vitepress.dev/). Source files are in `docs/` at the project root.

## Running locally

```bash
cd docs
pnpm install
pnpm dev
```

This starts a local dev server (default: `http://localhost:5173/dirsql/`). The site hot-reloads on file changes.

## Building

```bash
cd docs
pnpm build
```

The build must succeed before pushing. VitePress will fail on broken links, missing assets, and syntax errors in markdown.

## Testing changes

Before pushing any docs changes:

1. Run `pnpm build` in `docs/` and confirm it exits cleanly
2. Spot-check the built output with `pnpm preview`
3. Verify sidebar navigation, code blocks, and internal links render correctly

## Structure

The docs follow the [Diataxis](https://diataxis.fr/) framework:

- **Tutorials** (`getting-started.md`) -- learning-oriented lessons: the
  reader performs each step and sees a result; success is author-guaranteed
- **How-to Guides** (`guide/`) -- task-oriented recipes, named after a
  reader's goal
- **Reference** (`api/`) -- information-oriented, API details
- **Explanation** -- understanding-oriented design rationale. Its canonical
  home is the root `ARCHITECTURE.md` (there is no explanation page under
  `docs/` yet); #374 surfaces it on the site via an include page, the same
  mechanism `docs/migrations.md` uses for `MIGRATIONS.md`. Edit the root
  file, never a rendered include.

Working rules:

- **Facts live once, in Reference.** Tutorials and how-tos link to reference
  material; they never re-list constructor parameters or duplicate API tables.
- **A how-to opens with a 1-2 line goal/motivation statement.** Deep
  rationale (tradeoffs, alternatives considered, theory) moves to Explanation
  only when it is substantial enough to stand alone and is reused across
  pages. Do not manufacture stub pages for a paragraph of "why".
- **Tutorial vs how-to:** a tutorial is a lesson along a path the author
  guarantees (toy dataset, no branching, output shown at every step); a
  how-to serves a competent reader pursuing their own goal.

The **CLI** (`cli/`) is a self-contained section: a top-level `CLI` nav tab
plus a `CLI` group in the **single global sidebar**. There is intentionally
**no path-scoped `/cli/` sidebar key** -- a path-scoped sidebar swaps out the
whole tree and hides every other section while on a CLI page (#301; see the
comment above `sidebar` in `config.ts`). Everything a CLI user needs --
installation, running the server, `init`, the `.dirsql.toml` config file, and
the HTTP API -- lives under `cli/`. Do not move CLI pages back into `guide/`.

## Conventions

- **Lead with the use case.** Open each feature description with *why* a
  reader would reach for it before *how* it works. Don't frame a feature
  by what an adjacent feature can't do.
  *Don't:* "Persistence avoids the thing the default mode can't do..."
  *Do:* "Persistence keeps the SQLite index on disk between runs so large
  directories don't re-scan on every startup."
- Wrap `dirsql` in backticks in all prose text
- Use VitePress [code group](https://vitepress.dev/guide/markdown#code-groups) syntax (`::: code-group`) for multi-language examples with `Python`, `Rust`, and `TypeScript` tabs
- Internal links use relative paths (e.g., `./guide/tables.md`)
- The VitePress config is at `docs/.vitepress/config.ts`
- The site is deployed under the `/dirsql/` base path
