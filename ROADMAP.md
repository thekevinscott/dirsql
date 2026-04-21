# Roadmap

## 0.1.0 (released)

- Rust core: in-memory SQLite, file watching, directory scanning, glob matching, row diffing
- Python SDK: async-by-default API, DirSQL with ready()/query()/watch(), Table with DDL + extract lambda
- Publishing: PyPI, crates.io, npm (placeholder)
- Monorepo: packages/rust, packages/python, packages/ts
- CI: Rust test, Python test/lint, conditional releases, trusted publishing (OIDC)
- Docs: VitePress site, per-package READMEs, benchmarks (Criterion)

## 0.2.0

### Built-in file parsing (replaces extract lambda for common cases)

The extract lambda does three things: **split** (row boundaries), **navigate** (find data in structure), **project** (map fields to columns). The built-in parsing system decomposes these into declarative config.

#### Primitives

**`format`** -- How to parse file content. 7 values:

| Format | Split | Parse |
|---|---|---|
| `json` | One row if object, many if array | JSON |
| `jsonl` | One row per line | JSON per line |
| `csv` | One row per data line | Header row defines fields |
| `tsv` | One row per data line | Tab-separated, header row |
| `toml` | One row per file | TOML |
| `yaml` | One row if mapping, many if sequence | YAML |
| `frontmatter` | One row per file | YAML frontmatter + body as `body` column |

Format is **inferrable from glob extension**: `.json` -> `json`, `.jsonl`/`.ndjson` -> `jsonl`, `.csv` -> `csv`, `.toml` -> `toml`, `.yaml`/`.yml` -> `yaml`, `.md` -> `frontmatter`. Explicit `format` overrides inference.

**`each`** -- Optional dot-path to an array within the parsed document. For nested data:

```toml
[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "catalog/*.json"
each = "data.items"
# Parses {"data": {"items": [{name: "...", price: 1.0}, ...]}}
```

**`columns`** -- Optional mapping overrides. Only needed when column names differ from source keys, or for path-derived data:

```toml
[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, body TEXT, author TEXT)"
glob = "comments/{thread_id}/index.jsonl"
# thread_id captured from glob path, body and author from JSONL content
```

For non-path cases:

```toml
columns.display_name = "metadata.author.name"   # dot-path into nested object
```

**`extract`** -- Existing callable escape hatch (SDK-only, not in config files). Overrides `format` when provided. Handles the ~20% of cases that need custom logic: binary files, content transforms, conditional filtering, multi-format files.

#### Schema handling

- **Relaxed by default**: extra keys in content are ignored. Missing keys become NULL.
- **Strict mode** available: `strict = true` on a table to error on mismatches (current SDK behavior).

#### Simplest case -- zero extra config

```toml
[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT)"
glob = "posts/*.json"
# format inferred from .json, columns match JSON keys 1:1
```

#### Full example -- `.dirsql.toml`

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**"]

[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, body TEXT, author TEXT, resolved INTEGER)"
glob = "_comments/{thread_id}/index.jsonl"

[[table]]
ddl = "CREATE TABLE documents (title TEXT, draft INTEGER, body TEXT)"
glob = "**/index.md"

[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "catalog/*.json"
each = "data.items"

[[table]]
ddl = "CREATE TABLE metrics (date TEXT, requests INTEGER, errors INTEGER)"
glob = "logs/*.csv"
```

#### Implementation

New `parser` module in `packages/rust/` that takes `(format, each, columns, path, content)` and returns `Vec<HashMap<String, Value>>`. The Python SDK calls this Rust parser when `format` is provided, falls back to the Python `extract` callable when not.

### Rust consumer API

High-level `DirSQL` struct in `dirsql` with parity to the Python SDK:

```rust
let db = DirSQL::builder()
    .config("./.dirsql.toml")
    .build()?;
let db = DirSQL::builder()
    .root(".")
    .table(Table::new(ddl, glob).format(Format::Jsonl))
    .ignore(["node_modules/**"])
    .build()?;

let rows = db.query("SELECT * FROM comments")?;

for event in db.watch() {
    // insert/update/delete/error events
}
```

Foundation for the CLI tool. Enables Rust consumers without PyO3.

### CLI tool

Ships inside the consolidated `dirsql` crate behind an opt-in `cli` feature. Library consumers (`cargo add dirsql`) get zero CLI deps. CLI install is `cargo install dirsql --features cli`. The npm and PyPI packages ship the prebuilt Rust binary with a thin launcher; they are not separate implementations.

Running `dirsql` starts a long-lived HTTP server — no subcommands. The server exposes:

- `POST /query` — JSON in, JSON rows out
- `GET /events` — Server-Sent Events stream of row change events

Flags: `--config` (default `./.dirsql.toml`), `--host` (default `localhost`), `--port` (default `7117`). One-shot `query` / `watch` / `init` subcommands were deliberately deferred — see `docs/guide/cli.md` for rationale.

### Python SDK: config file support

```python
# Read from .dirsql.toml (no lambdas needed)
db = DirSQL(config="./.dirsql.toml")

# Or mix: config file tables + programmatic tables with extract
db = DirSQL("/path", tables=[...], config=".dirsql.toml")
```

## 0.3.0

### Persistent SQLite

Optional on-disk SQLite database instead of in-memory. Survives restarts and uses git's racy-stat algorithm to skip re-parsing files whose filesystem metadata matches the cached state.

```toml
[dirsql]
persist = true                       # default: false (in-memory)
persist_path = ".dirsql/cache.db"    # optional; this is the default
```

User contract: **the rows returned by `query()` after startup are equivalent to those produced by a from-scratch rebuild** — persistence is a startup-time optimization, not a correctness compromise. See [docs/guide/persistence.md](docs/guide/persistence.md) for the full algorithm and edge-case discussion.

#### Storage layout

When `persist = true` (and no explicit `persist_path` is set), `dirsql` writes to `<root>/.dirsql/cache.db`. The `.dirsql/` directory is **reserved**: it is unconditionally excluded from the directory walk whether persistence is enabled or not. Any other file inside `.dirsql/` is ignored by the scanner.

#### Sidecar schema

Two metadata tables live alongside the user-defined data tables:

```sql
CREATE TABLE _dirsql_files (
  path              TEXT    PRIMARY KEY,
  table_name        TEXT    NOT NULL,
  size              INTEGER NOT NULL,
  mtime_ns          INTEGER NOT NULL,
  ctime_ns          INTEGER NOT NULL,
  inode             INTEGER NOT NULL,
  dev               INTEGER NOT NULL,
  content_hash      BLOB    NOT NULL,   -- BLAKE3, 32 bytes
  snapshot_time_ns  INTEGER NOT NULL    -- when this row was last written
);

CREATE TABLE _dirsql_meta (
  key    TEXT PRIMARY KEY,
  value  TEXT NOT NULL
);
-- seeded with: schema_version, dirsql_version, glob_config_hash,
-- parser_versions (JSON), root_canonical
```

#### Reconcile algorithm (git racy-stat, adapted)

On startup, when a persistent cache exists:

1. Read `_dirsql_meta`. If any of `schema_version`, `dirsql_version`, `glob_config_hash`, `parser_versions`, or `root_canonical` differs from the current build, **wipe everything and rebuild from scratch** — no partial invalidation.
2. Walk the tree, `stat` every file matching a configured glob.
3. For each file, classify by comparing the live stat tuple to the row in `_dirsql_files`:
   - **Trust the cache** when `(size, mtime_ns, ctime_ns, inode, dev)` matches *and* `mtime_ns < snapshot_time_ns - epsilon` (outside the racy window).
   - **Hash-confirm** when the tuple matches but `mtime_ns >= snapshot_time_ns - epsilon`. If the hash matches, trust the cache and update `snapshot_time_ns`. If it differs, re-parse.
   - **Re-parse** when any field of the tuple differs.
   - **Delete** rows for files in `_dirsql_files` that are not present on disk.
   - **Insert** rows for files on disk that are not in `_dirsql_files`.
4. Commit the transaction.

#### Hashing

BLAKE3, 32-byte digest stored as `BLOB`. Used for both per-file content hashes (`_dirsql_files.content_hash`) and the glob-config fingerprint in `_dirsql_meta`.

#### Limitations

- **Network filesystems (NFS, SMB):** attribute caching can produce stale `stat` results. Behavior on these filesystems is undefined for v0.3.0; document a warning and recommend in-memory mode. Auto-detection via `statfs` is a follow-up.
- **mtime-preserving in-place edit with identical size:** the only failure mode of racy-stat. Requires `touch -r` after editing while preserving byte length. Documented; users who need stronger guarantees should disable persistence.

Enables:
- Fast startup for large directories (no full re-parse)
- External tools querying the SQLite file directly
- Backup/restore of index state

### TypeScript SDK (`packages/ts/`)

napi-rs bindings to `dirsql`. Same API shape as Python:

```typescript
const db = new DirSQL({ root, tables, ignore });

const rows = await db.query("SELECT * FROM comments");

for await (const event of db.watch()) {
  console.log(event.action, event.table, event.row);
}
```

Or config-based (string = config file path):

```typescript
const db = new DirSQL("./.dirsql.toml");
```

### LLM-assisted schema inference

One-time LLM call to inspect directory structure and generate `.dirsql.toml`. The LLM examines file samples and proposes table definitions. Output is a static config file -- no LLM in the hot path.

```bash
dirsql init --infer    # uses LLM to generate .dirsql.toml from directory contents
```

## Future

- **Linux aarch64 wheels**: cross-compilation for manylinux aarch64 (currently disabled)
- **Query caching**: cache frequent query results, invalidate on file change
- **Webhooks**: POST to a URL on file change events
- **Schema migrations**: handle column additions/removals gracefully when file schemas evolve
- **Plugin system**: custom format parsers as shared libraries or WASM modules
