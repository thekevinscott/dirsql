# Roadmap

## 0.1.0 (released)

- Rust core: in-memory SQLite, file watching, directory scanning, glob matching, row diffing
- Python SDK: async-by-default API, DirSQL with ready()/query()/watch(), Table with DDL + extract lambda
- Publishing: PyPI, crates.io, npm (placeholder)
- Monorepo: packages/core, packages/python, packages/ts
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

New `parser` module in `packages/core/` that takes `(format, each, columns, path, content)` and returns `Vec<HashMap<String, Value>>`. The Python SDK calls this Rust parser when `format` is provided, falls back to the Python `extract` callable when not.

### Rust consumer API

High-level `DirSQL` struct in `dirsql-core` with parity to the Python SDK:

```rust
let db = DirSQL::from_config(".")?;  // reads .dirsql.toml
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

### CLI tool (`packages/cli/`)

Separate binary crate depending on `dirsql-core`.

```bash
dirsql init                              # create .dirsql.toml with examples
dirsql query "SELECT * FROM comments"    # one-shot query
dirsql serve                             # long-running, HTTP + file watching
dirsql watch                             # stream events to stdout as JSONL
```

- `dirsql serve` exposes HTTP API: POST `/query` for SQL, GET `/events` (SSE) for change stream
- Optional Unix socket for local-only access
- Reads `.dirsql.toml` by default, override with `--config`

### Python SDK: config file support

```python
# Read from .dirsql.toml (no lambdas needed)
db = DirSQL.from_config(".")

# Or mix: config file tables + programmatic tables with extract
db = DirSQL("/path", tables=[...], config=".dirsql.toml")
```

## 0.3.0

### Persistent SQLite

Optional on-disk SQLite database instead of in-memory. Survives restarts -- only re-indexes changed files on startup (compare mtime/hash against stored state).

```toml
[dirsql]
persist = true                    # default: false (in-memory)
db_path = ".dirsql/index.sqlite"  # default location
```

Enables:
- Fast startup for large directories (no full re-scan)
- External tools querying the SQLite file directly
- Backup/restore of index state

### TypeScript SDK (`packages/ts/`)

napi-rs bindings to `dirsql-core`. Same API shape as Python:

```typescript
const db = new DirSQL(root, { tables, ignore });

const rows = await db.query("SELECT * FROM comments");

for await (const event of db.watch()) {
  console.log(event.action, event.table, event.row);
}
```

Or config-based:

```typescript
const db = DirSQL.fromConfig(".");
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
