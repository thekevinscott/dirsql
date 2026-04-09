# dirsql-sdk

Rust SDK for `dirsql`. Wraps the `dirsql-core` engine in a user-facing API that mirrors the Python SDK where Rust idioms allow.

## Installation

```bash
cargo add dirsql-sdk
```

## API

- `Table::new(...)` for infallible extractors
- `Table::try_new(...)` for fallible extractors
- `DirSQL::new(...)` for synchronous indexing
- `DirSQL::with_ignore(...)` for indexing with ignore patterns
- `DirSQL::query(...)` for SQL queries
- `DirSQL::watch(...)` for an async row-event stream

## License

MIT
