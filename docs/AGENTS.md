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

The docs follow the [Diataxis](https://diataxis.fr/) framework. **Type is
the only organizational axis** -- there are no product-area sections (no
"CLI" section; #353). The nav and the sidebar mirror the four types exactly.

The **primary reader is the CLI user**: someone with a directory of files,
one command (`uvx` / `npx dirsql`), and a `.dirsql.toml`. The SDKs are the
secondary audience and appear **only in Reference**, plus the single
"Embed `dirsql` in your application" how-to.

Target tree (the spec for #353; existing pages are *quarried* into it, not
migrated -- a page survives only if a slot wants its content):

- **Tutorial** (`getting-started.md`) -- one lesson: *Your first dirsql
  database*. The reader performs every step and sees output at each one;
  success is author-guaranteed (toy dataset, no branching).
- **How-to Guides** (`howto/`) -- goal-named recipes: define tables for your files;
  derive columns from file paths; extract rows from file contents
  (`on-file`); search documents by meaning; skip files; load a SQLite
  extension; keep the index across restarts; react to file changes; embed
  `dirsql` in an application.
- **Reference** (`reference/`) -- CLI flags and defaults; the complete
  `.dirsql.toml` schema; the command hook contract (placeholders, stdout
  protocol, exit codes, timeouts); virtual columns and glob captures; the
  HTTP API; the SDK page (`reference/sdk.md`, one page with
  Python/TypeScript/Rust code-groups -- the sole SDK home); plus the
  Migrations include (`migrations.md`).
- **Explanation** (`explanation.md`) -- one page: how `dirsql` thinks (the
  filesystem is the source of truth; the database is a derived, ephemeral,
  read-only view; reconcile and diffing). Its canonical home is the root
  `ARCHITECTURE.md`;
  #374 surfaces it via an include page, the same mechanism
  `docs/migrations.md` uses for `MIGRATIONS.md`. Edit the root file, never
  a rendered include.

Working rules:

- **Facts live once, in Reference.** Tutorials and how-tos link to reference
  material; they never re-list constructor parameters or duplicate API tables.
- **A how-to opens with a 1-2 line goal/motivation statement.** Deep
  rationale (tradeoffs, alternatives considered, theory) moves to Explanation
  only when it is substantial enough to stand alone and is reused across
  pages. Do not manufacture stub pages for a paragraph of "why".
- **Tutorial vs how-to:** a tutorial is a lesson along a path the author
  guarantees; a how-to serves a competent reader pursuing their own goal.
- **One global sidebar.** Never add a path-scoped sidebar key -- it swaps
  out the whole tree and hides every other section while inside one (#301;
  see the comment above `sidebar` in `config.ts`).

## Conventions

- **Lead with the use case.** Open each feature description with *why* a
  reader would reach for it before *how* it works. Don't frame a feature
  by what an adjacent feature can't do.
  *Don't:* "Persistence avoids the thing the default mode can't do..."
  *Do:* "Persistence keeps the SQLite index on disk between runs so large
  directories don't re-scan on every startup."
- Wrap `dirsql` in backticks in all prose text
- Use VitePress [code group](https://vitepress.dev/guide/markdown#code-groups) syntax (`::: code-group`) for multi-language examples with `Python`, `Rust`, and `TypeScript` tabs
- Internal links use relative paths (e.g., `./howto/define-tables.md`)
- The VitePress config is at `docs/.vitepress/config.ts`
- The site is deployed under the `/dirsql/` base path
