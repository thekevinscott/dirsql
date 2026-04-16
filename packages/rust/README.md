# dirsql

Rust SDK for `dirsql`. Ephemeral SQL index over a local directory.

## Installation

```bash
cargo add dirsql
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
