# Configuration File

`dirsql` can be configured with a `.dirsql.toml` file, allowing you to define tables declaratively without writing code.

## Basic Example

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**"]

[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT)"
glob = "posts/*.json"
```

The `format` is inferred from the glob extension (`.json` -> JSON, `.jsonl` -> JSONL, `.csv` -> CSV, etc.). Each JSON key maps to a column with the same name.

## Supported Formats

| Extension | Format | Rows |
|---|---|---|
| `.json` | JSON | Object = 1 row, Array = many rows |
| `.jsonl`, `.ndjson` | JSONL | One row per line |
| `.csv` | CSV | One row per data line (header = columns) |
| `.tsv` | TSV | One row per data line (tab-separated) |
| `.toml` | TOML | One row per file |
| `.yaml`, `.yml` | YAML | Mapping = 1 row, Sequence = many rows |
| `.md` | Frontmatter | YAML frontmatter + body column |

## Path Captures

Use `{name}` in glob patterns to extract path segments as columns:

```toml
[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, body TEXT, author TEXT)"
glob = "_comments/{thread_id}/index.jsonl"
```

The directory name (e.g., `abc123`) becomes the `thread_id` column value for every row in that file.

## Nested Data

Use `each` to navigate into nested JSON structures:

```toml
[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "catalog/*.json"
each = "data.items"
```

This extracts rows from `{"data": {"items": [...]}}`.

## Column Mapping

Use `columns` to map SQL column names to nested fields or path captures:

```toml
[[table]]
ddl = "CREATE TABLE posts (display_name TEXT, body TEXT)"
glob = "posts/*.json"

[table.columns]
display_name = "metadata.author.name"
```

## Ignore Patterns

The `ignore` list skips files and directories entirely (not even scanned):

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**", "*.pyc", "__pycache__/**"]
```

## Strict Mode

By default, extra keys in file content are ignored and missing keys become NULL. Enable strict mode to error on mismatches:

```toml
[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT)"
glob = "posts/*.json"
strict = true
```

## Full Example

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**", "dist/**"]

[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, body TEXT, author TEXT, resolved INTEGER)"
glob = "_comments/{thread_id}/index.jsonl"

[[table]]
ddl = "CREATE TABLE documents (title TEXT, draft INTEGER, body TEXT)"
glob = "**/index.md"

[[table]]
ddl = "CREATE TABLE metrics (date TEXT, requests INTEGER, errors INTEGER)"
glob = "logs/*.csv"

[[table]]
ddl = "CREATE TABLE config (key TEXT, value TEXT)"
glob = "config/*.toml"
strict = true
```
