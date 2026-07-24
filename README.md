# `dirsql`

**Turn a filesystem into a database**

Ephemeral SQL index over a local directory. `dirsql` watches a filesystem, ingests structured files into an ephemeral SQLite database, and exposes a SQL query interface. On shutdown the database is discarded -- the filesystem remains the source of truth.

**`dirsql` never modifies your files.** It opens them for reading and nothing else -- no writes, no moves, no deletes, no rewrites in place. Point it at anything, including a directory you have not backed up, and the worst it can do is read. This is a permanent design guarantee, not a feature that has yet to be built; see [Read-only by design](ARCHITECTURE.md#read-only-by-design) for its exact scope.

The full documentation lives in [`docs/`](docs/) and is published at <https://thekevinscott.github.io/dirsql/>. This README mirrors the layout of `docs/` (every section below maps to a page) so agents and humans reading the source can navigate without leaving the repo. Each section is the bare minimum -- click through for the full guide.

## Why

A folder of files is durable, diff-able, version-controllable, and legible without `dirsql` running. A SQL database is fast to query, easy to join across, and ergonomic for filtering. `dirsql` bridges the two: the filesystem stays the source of truth, and you get a SQL index over it for free.

`dirsql` is a queryable index over a filesystem. Files are rows; columns come from filesystem facts (the path, glob captures like `posts/{thread_id}/*.md`, and stat metadata). Content interpretation — parsing markdown frontmatter, JSON, CSV, and the like — is intentionally not dirsql's job; if you need that, register a programmatic table whose on-file callback does the parsing.

## Installation

```bash
pip install dirsql                 # Python
cargo add dirsql                   # Rust (library)
npm add dirsql                     # TypeScript

# CLI: query is the default (`dirsql server` starts the HTTP server)
npx dirsql "SELECT * FROM './'"    # via npm
uvx dirsql "SELECT * FROM './'"    # via PyPI
cargo install dirsql --features cli
```

> The npm CLI requires **Node ≥ 20.11**.

## Quick start

```python
import asyncio, json
from dirsql import DirSQL, Table

async def main():
    db = DirSQL(
        "./my-blog",
        tables=[
            Table(
                ddl="CREATE TABLE posts (title TEXT, author TEXT)",
                glob="posts/*.json",
                on_file=lambda path: [json.loads(open(path, encoding="utf-8").read())],
            ),
        ],
    )
    await db.ready()
    print(await db.query("SELECT * FROM posts"))

asyncio.run(main())
```

Rust and TypeScript versions are in [`docs/howto/embed.md`](docs/howto/embed.md).

## Tutorial

*Your first dirsql database* -- a hands-on lesson: create a toy dataset, query it with a single command and zero configuration, then declare a table to name and reuse a shape. The reader performs every step and sees output at each one.

→ [`docs/getting-started.md`](docs/getting-started.md)

## How-to Guides

Goal-named recipes for everyday `dirsql` use:

- [Define tables for your files](docs/howto/define-tables.md)
- [Derive columns from file paths](docs/howto/columns-from-paths.md)
- [Extract rows from file contents](docs/howto/extract-from-contents.md)
- [Search documents by meaning](docs/howto/search-by-meaning.md)
- [Skip files you don't want indexed](docs/howto/skip-files.md)
- [Load a SQLite extension](docs/howto/load-extension.md)
- [Keep the index across restarts](docs/howto/persist.md)
- [React to file changes](docs/howto/react-to-changes.md)
- [Embed `dirsql` in your application](docs/howto/embed.md)

## Reference

The canonical facts -- flags, schemas, contracts, and API shapes:

- [CLI](docs/reference/cli.md) -- flags, `dirsql init`, defaults, exit codes
- [Configuration file](docs/reference/config.md) -- the complete `.dirsql.toml` schema
- [Command hooks](docs/reference/hooks.md) -- placeholders, stdout protocol, exit codes, timeouts
- [Stat columns & glob captures](docs/reference/columns.md) -- `path`, `basename`, `dir`, `ext`, `size`, `mtime`, `ctime`, and `{name}` captures
- [HTTP API](docs/reference/http-api.md) -- `POST /query`, `GET /events`, errors
- [SDK](docs/reference/sdk.md) -- `DirSQL`, `Table`, and `RowEvent` across Python, Rust, and TypeScript
- [Migrations](docs/migrations.md) -- upgrade notes for breaking changes; the canonical source is [`MIGRATIONS.md`](MIGRATIONS.md) at the repo root

## Explanation

How `dirsql` thinks: the filesystem is the source of truth; the database is a derived, ephemeral, read-only view. The canonical source is [`ARCHITECTURE.md`](ARCHITECTURE.md) at the repo root.

→ [`docs/explanation.md`](docs/explanation.md)

## Architecture

Monorepo with three published packages, all named `dirsql`:

- [`packages/rust/`](packages/rust/) -- Rust SDK and core engine. SQLite indexing, filesystem scanning, glob matching, file watching, row diffing. Published to crates.io.
- [`packages/python/`](packages/python/) -- Python SDK over PyO3 with an async wrapper. Published to PyPI.
- [`packages/ts/`](packages/ts/) -- TypeScript SDK over napi-rs. Published to npm.

Cross-language constraints, the one-implementation principle, and SDK design live in [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Development

- Workflow rules: [`AGENTS.md`](AGENTS.md)
- Architecture: [`ARCHITECTURE.md`](ARCHITECTURE.md)
- Cross-SDK parity tracker: [`PARITY.md`](PARITY.md)
- Roadmap: [`ROADMAP.md`](ROADMAP.md)

```bash
just ci               # all checks
just test-rust        # Rust unit tests
just test-integration # Python integration tests
just clippy           # Rust lints
just lint             # Python lints (ruff)
```

## License

MIT
