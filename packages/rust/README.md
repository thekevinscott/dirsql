# dirsql

Rust crate for `dirsql`. Ephemeral SQL index over a local directory — watch a filesystem, ingest structured files into an in-memory SQLite database, and query them with SQL.

[Documentation](https://thekevinscott.github.io/dirsql/?lang=rust)

Also available as [`dirsql` on PyPI](https://pypi.org/project/dirsql/) and [`dirsql` on npm](https://www.npmjs.com/package/dirsql).

## Install as a library

```bash
cargo add dirsql
```

```rust
use dirsql::{DirSQL, Table};

let db = DirSQL::new(
    "./my-project",
    vec![
        Table::new(
            "CREATE TABLE posts (title TEXT, author TEXT)",
            "posts/*.json",
            |_path, content| vec![serde_json::from_str(content).unwrap()],
        ),
    ],
)?;

let rows = db.query("SELECT * FROM posts")?;
```

See the [documentation site](https://thekevinscott.github.io/dirsql/) for the full library API and configuration reference.

## Install as a CLI

```bash
cargo install dirsql --features cli
dirsql
```

Running `dirsql` starts an HTTP server bound to `localhost:7117` that exposes the SDK over HTTP: `POST /query` for SQL and `GET /events` for a Server-Sent Events change stream. Override with `--host`, `--port`, `--config`.

The `cli` feature is **opt-in** — `cargo add dirsql` pulls no CLI dependencies. `cargo install dirsql` without `--features cli` silently installs nothing (`required-features` skips the bin target with no warning); always include the flag, or use `npx dirsql` / `uvx dirsql` for prebuilt binaries.

## Feature flags

| Feature | Default | Description |
|---|---|---|
| `cli` | no | Enables the `dirsql` binary and its dependencies. |

## License

MIT
