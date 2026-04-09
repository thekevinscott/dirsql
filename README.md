# dirsql

Ephemeral SQL index over a local directory. Watches a filesystem, ingests structured files into an in-memory SQLite database, and exposes a SQL query interface. On shutdown, the database is discarded -- the filesystem remains the source of truth.

## Why

Structured data stored as flat files (JSONL, JSON) is easy to read, write, diff, and version. But querying across many files is slow -- "show me all unresolved comments across 50 documents" requires opening and parsing every file.

dirsql bridges this gap: files remain the source of truth, but you get SQL queries and real-time change events for free.

## Architecture

The project is a monorepo with three packages:

- **[`packages/core/`](packages/core/)** -- Rust core library (`dirsql-core`). Handles SQLite, filesystem scanning, glob matching, file watching, and row diffing. All heavy lifting happens here.
- **[`packages/rust/`](packages/rust/)** -- Rust SDK (`dirsql-sdk`). User-facing Rust API over the core engine with parity to the Python SDK where Rust idioms allow.
- **[`packages/python/`](packages/python/)** -- Python SDK (`dirsql`). PyO3 bindings to the Rust core, plus a pure-Python async wrapper. Published to PyPI.
- **[`packages/ts/`](packages/ts/)** -- TypeScript SDK (not yet implemented).

```
                  ┌──────────────┐
                  │  dirsql-core │   Rust: rusqlite + notify + walkdir
                  │  (packages/  │
                  │    core/)    │
                  └──────┬───────┘
                         │
              ┌──────────┼──────────┐
              │ PyO3     │          │ (future: napi-rs)
     ┌────────▼───────┐ ┌────▼─────────┐ ┌──────▼───────┐
     │  Python SDK    │ │  Rust SDK    │ │   TS SDK     │
     │  (packages/    │ │ (packages/   │ │  (packages/  │
     │    python/)    │ │   rust/)     │ │    ts/)      │
     └────────────────┘ └──────────────┘ └──────────────┘
                         │
```

## SDK Documentation

- **Python**: [packages/python/README.md](packages/python/README.md) -- installation, sync + async API, full reference
- **Rust**: [packages/core/README.md](packages/core/README.md) -- crate usage and module overview
- **TypeScript**: [packages/ts/README.md](packages/ts/README.md) -- status and roadmap

## Development

### Prerequisites

- Rust (stable)
- Python >= 3.12
- [maturin](https://github.com/PyO3/maturin) for building the Python extension
- [just](https://github.com/casey/just) as a task runner

### Build and Test

```bash
# Build the Python extension (dev mode)
maturin develop

# Run all CI checks
just ci

# Individual targets
just test-rust        # Rust unit tests
just test-integration # Python integration tests
just clippy           # Rust lints
just lint             # Python lints (ruff)
```

## License

MIT
