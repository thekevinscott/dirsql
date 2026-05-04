---
canonical: https://thekevinscott.github.io/dirsql/guide/init
---

# Generating a config with `dirsql init`

> Online: <https://thekevinscott.github.io/dirsql/guide/init>

`dirsql init` generates a `.dirsql.toml` by running `claude` over the target directory.

## Examples

### Mixed files

```
my-downloads/
├── archive.zip
├── invoice.pdf
├── notes.txt
└── photo.jpg
```

When no structured format is detected, `dirsql init` falls back to a metadata-only table:

```toml
[[table]]
ddl = "CREATE TABLE files (path TEXT, ext TEXT, size INTEGER)"
glob = "*"
```

### Flat directory

```
my-expenses/
├── coffee.json
├── lunch.json
└── flight.json
```

Each file is a JSON object like `{"amount": 4.50, "vendor": "Blue Bottle", "date": "2025-04-12"}`.

```toml
[[table]]
ddl = "CREATE TABLE expenses (amount REAL, vendor TEXT, date TEXT)"
glob = "*.json"
```

### Subdirectories with path captures

```
my-blog/
├── posts/
│   ├── hello-world.json
│   └── second.json
└── _comments/
    └── hello-world/
        └── index.jsonl
```

```toml
[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT, draft INTEGER)"
glob = "posts/*.json"

[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, author TEXT, body TEXT)"
glob = "_comments/{thread_id}/index.jsonl"
```

`init` will not overwrite an existing config without `--force`.

## Flags

| Flag | Default | Description |
|---|---|---|
| `--root <path>` | cwd | Directory to scan |
| `--output <path>` | `<root>/.dirsql.toml` | Output path |
| `--force` | off | Overwrite if the output exists |

## Authentication

Requires `claude` on `PATH` and signed in. There is no separate API key. If `claude` is missing, `dirsql init` raises an exception.
