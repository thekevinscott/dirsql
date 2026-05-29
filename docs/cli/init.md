---
canonical: https://thekevinscott.github.io/dirsql/cli/init
---

# Generating a config with `dirsql init`

> Online: <https://thekevinscott.github.io/dirsql/cli/init>

`dirsql init` generates a [`.dirsql.toml`](./config.md) by running `claude` over the target directory.

The output is limited to filesystem-fact tables. For content-aware schemas, see [Defining Tables](../guide/tables.md).

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
name = "files"
glob = "*"

  [[table.column]]
  name = "_path"
  type = "TEXT"

  [[table.column]]
  name = "_ext"
  type = "TEXT"

  [[table.column]]
  name = "_size"
  type = "INTEGER"
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
name = "photos"
glob = "{month}/*.jpg"

  [[table.column]]
  name = "month"
  type = "TEXT"

  [[table.column]]
  name = "_basename"
  type = "TEXT"

  [[table.column]]
  name = "_mtime"
  type = "INTEGER"
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
name = "posts"
glob = "posts/*.md"

  [[table.column]]
  name = "_basename"
  type = "TEXT"

  [[table.column]]
  name = "_mtime"
  type = "INTEGER"

  [[table.column]]
  name = "_size"
  type = "INTEGER"

[[table]]
name = "comments"
glob = "_comments/{thread_id}/*.jsonl"

  [[table.column]]
  name = "thread_id"
  type = "TEXT"

  [[table.column]]
  name = "_basename"
  type = "TEXT"

  [[table.column]]
  name = "_mtime"
  type = "INTEGER"
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
