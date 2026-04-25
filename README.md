# `dirsql`

Ephemeral SQL index over a local directory. `dirsql` watches a filesystem, ingests structured files into an in-memory SQLite database, and exposes a SQL query interface. On shutdown the database is discarded -- the filesystem remains the source of truth.

The full documentation lives in [`docs/`](docs/) and is published at <https://thekevinscott.github.io/dirsql/>. This README mirrors the layout of `docs/` (every section below maps to a page) so agents and humans reading the source can navigate without leaving the repo. Each section is the bare minimum -- click through for the full guide.

## Why

Structured data stored as flat files (JSON, JSONL, CSV, markdown) is easy to read, write, diff, and version-control. But querying across many files is slow -- "show me all records matching X across 50 files" requires opening and parsing every file.

`dirsql` bridges this gap. The filesystem stays the source of truth; you get SQL queries and real-time change events for free. Define tables with a glob pattern and an extract function, and `dirsql` handles the rest.

## Installation

```bash
pip install dirsql                 # Python
cargo add dirsql                   # Rust (library)
pnpm add dirsql                    # TypeScript

# CLI (HTTP server, identical functionality)
npx dirsql                         # via npm
uvx dirsql                         # via PyPI
cargo install dirsql --features cli
```

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
                extract=lambda path, content: [json.loads(content)],
            ),
        ],
    )
    await db.ready()
    print(await db.query("SELECT * FROM posts"))

asyncio.run(main())
```

Rust and TypeScript versions are in [`docs/getting-started.md`](docs/getting-started.md).

## Getting Started

End-to-end walkthrough: install, define tables, scan a directory, run queries, and join across tables.

→ [`docs/getting-started.md`](docs/getting-started.md)

## Guide

Task-oriented recipes for everyday `dirsql` use.

### Configuration File

Declare tables, ignore patterns, and the scan root in a `.dirsql.toml` file -- no code required. Covers path captures (`{name}`), nested-data extraction (`each`), column mapping, supported formats, and strict mode.

→ [`docs/guide/config.md`](docs/guide/config.md)

### Defining Tables

A table is a `(ddl, glob, extract)` triple. The DDL defines the SQLite schema, the glob selects files, and the extract function turns each file into rows.

→ [`docs/guide/tables.md`](docs/guide/tables.md)

### Querying

`db.query(sql)` accepts any read-only SQLite SELECT (subqueries, CTEs, window functions, joins) and returns rows as dicts/maps/objects keyed by column name. Writes (`INSERT`, `UPDATE`, `DELETE`, ...) are rejected -- mutate the underlying files instead.

→ [`docs/guide/querying.md`](docs/guide/querying.md)

### File Watching

`db.watch()` returns an async iterable of `RowEvent`s (`insert` / `update` / `delete` / `error`) as files change on disk. `dirsql` diffs the previous and current rows of each file to produce row-level events.

→ [`docs/guide/watching.md`](docs/guide/watching.md)

### Async API

In Python, `DirSQL` is async by default: the constructor returns immediately, scanning runs in a background thread, and `ready()` / `query()` / `watch()` integrate with `asyncio`. Rust uses `AsyncDirSQL` under tokio; TypeScript awaits `db.ready` and `db.query()`.

→ [`docs/guide/async.md`](docs/guide/async.md)

### Command-Line Interface

`dirsql` runs an HTTP server (default `localhost:7117`) exposing `POST /query` for SQL and `GET /events` for an SSE change stream. Same SDK functionality, language-agnostic transport.

→ [`docs/guide/cli.md`](docs/guide/cli.md)

### Collaboration with CRDTs

For multi-writer / local-first workflows, pair `dirsql` with [Automerge](https://automerge.org/): the CRDT owns merge semantics, `dirsql` indexes the materialized JSON view. Includes the integration shape, tradeoffs vs plain files, and notes on Yjs / Loro.

→ [`docs/guide/crdt.md`](docs/guide/crdt.md)

## API Reference

`DirSQL`, `Table`, and `RowEvent` -- constructors, methods, and field-by-field shapes across Python, Rust, and TypeScript.

→ [`docs/api/index.md`](docs/api/index.md)

## Migrations

Upgrade notes for breaking changes. The canonical source is [`MIGRATIONS.md`](MIGRATIONS.md) at the repo root; the docs site renders it via include.

→ [`docs/migrations.md`](docs/migrations.md)

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
