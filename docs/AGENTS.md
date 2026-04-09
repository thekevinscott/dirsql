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

- **Tutorials** (`getting-started.md`) -- learning-oriented, step-by-step
- **How-to Guides** (`guide/`) -- task-oriented, practical recipes
- **Reference** (`api/`) -- information-oriented, API details
- **Explanation** (`architecture.md`) -- understanding-oriented, design decisions

## Conventions

- Wrap `dirsql` in backticks in all prose text
- Use VitePress [code group](https://vitepress.dev/guide/markdown#code-groups) syntax (`::: code-group`) for multi-language examples with `Python`, `Rust`, and `TypeScript` tabs
- Internal links use relative paths (e.g., `./guide/tables.md`)
- The VitePress config is at `docs/.vitepress/config.ts`
- The site is deployed under the `/dirsql/` base path
