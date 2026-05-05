---
canonical: https://thekevinscott.github.io/dirsql/guide/init
---

# Generating a config with `dirsql init`

> Online: <https://thekevinscott.github.io/dirsql/guide/init>

`dirsql init` generates a `.dirsql.toml` by running `claude` over the target directory.

The output is limited to filesystem-fact tables. For content-aware schemas, see [Defining Tables](./tables.md).

## Examples

### Mixed files

```
my-downloads/
├── archive.zip
├── invoice.pdf
├── notes.txt
└── photo.jpg
```

```toml
[[table]]
ddl  = "CREATE TABLE files (_path TEXT, _ext TEXT, _size INTEGER)"
glob = "*"
```

### Path captures

```
photos/
├── 2024-01/
│   ├── beach.jpg
│   └── sunset.jpg
└── 2024-02/
    ├── snow.jpg
    └── mountain.jpg
```

```toml
[[table]]
ddl  = "CREATE TABLE photos (month TEXT, _basename TEXT, _mtime INTEGER)"
glob = "{month}/*.jpg"
```

### Multiple tables

```
my-blog/
├── posts/
│   ├── hello-world.md
│   └── second.md
└── _comments/
    └── hello-world/
        ├── 2024-01-15.jsonl
        └── 2024-02-03.jsonl
```

```toml
[[table]]
ddl  = "CREATE TABLE posts (_basename TEXT, _mtime INTEGER, _size INTEGER)"
glob = "posts/*.md"

[[table]]
ddl  = "CREATE TABLE comments (thread_id TEXT, _basename TEXT, _mtime INTEGER)"
glob = "_comments/{thread_id}/*.jsonl"
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
