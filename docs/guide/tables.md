# Defining Tables

Each table in dirsql maps a set of files to rows in an in-memory SQLite table. A table definition has three parts: DDL, a glob pattern, and an extract function.

## Table constructor

```python
from dirsql import Table

table = Table(
    ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
    glob="comments/**/index.jsonl",
    extract=lambda path, content: [
        {"id": "...", "body": "...", "author": "..."}
    ],
)
```

All three arguments are keyword-only.

### `ddl`

A SQLite `CREATE TABLE` statement. This defines the schema of the table. dirsql executes this DDL directly against the in-memory database, so any valid SQLite column types and constraints work.

```python
# Simple text columns
ddl="CREATE TABLE notes (title TEXT, body TEXT)"

# Typed columns
ddl="CREATE TABLE metrics (name TEXT, value REAL, count INTEGER)"

# With constraints
ddl="CREATE TABLE items (id TEXT PRIMARY KEY, name TEXT NOT NULL)"
```

The table name is parsed from the DDL. It must be a valid SQLite identifier.

### `glob`

A glob pattern that determines which files feed into this table. Matched relative to the root directory passed to `DirSQL`.

```python
glob="*.json"                  # JSON files in root only
glob="**/*.json"               # JSON files at any depth
glob="comments/**/index.jsonl" # JSONL files in comment subdirectories
glob="data/*.csv"              # CSV files in data/
```

Glob syntax follows standard Unix globbing rules. `**` matches any number of directory levels.

### `extract`

A callable `(path: str, content: str) -> list[dict]` that converts a file into rows.

- `path` is the file path relative to the root directory
- `content` is the file content as a string
- Return a list of dicts, where each dict maps column names to values
- Return an empty list to skip a file

```python
import json

# Single-object JSON files: one row per file
extract=lambda path, content: [json.loads(content)]

# JSONL files: one row per line
extract=lambda path, content: [
    json.loads(line) for line in content.splitlines()
]

# Derive values from the file path
import os
extract=lambda path, content: [
    {
        "id": os.path.basename(os.path.dirname(path)),
        "body": json.loads(line)["body"],
    }
    for line in content.splitlines()
    for _ in [json.loads(line)]
]

# Conditionally skip files
def extract(path, content):
    data = json.loads(content)
    if data.get("draft"):
        return []
    return [data]
```

## Multiple tables

Pass multiple `Table` definitions to index different file types into separate tables:

```python
from dirsql import DirSQL, Table
import json

db = DirSQL(
    "./workspace",
    tables=[
        Table(
            ddl="CREATE TABLE posts (title TEXT, author_id TEXT)",
            glob="posts/*.json",
            extract=lambda path, content: [json.loads(content)],
        ),
        Table(
            ddl="CREATE TABLE authors (id TEXT, name TEXT)",
            glob="authors/*.json",
            extract=lambda path, content: [json.loads(content)],
        ),
    ],
)
```

Each table has its own glob and extract function. A file can only match one table (the first matching glob wins).

## Ignore patterns

Use the `ignore` parameter to exclude paths from all tables:

```python
db = DirSQL(
    "./workspace",
    ignore=["**/node_modules/**", "**/.git/**"],
    tables=[...],
)
```

Ignore patterns are applied before glob matching. Any file matching an ignore pattern is skipped regardless of table globs.

## Supported value types

The extract function can return these Python types, which map to SQLite types:

| Python type | SQLite type |
|-------------|-------------|
| `str`       | TEXT        |
| `int`       | INTEGER     |
| `float`     | REAL        |
| `bool`      | INTEGER (0/1) |
| `bytes`     | BLOB        |
| `None`      | NULL        |

Any other type is converted to its string representation via `str()`.
