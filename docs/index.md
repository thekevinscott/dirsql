---
layout: home
hero:
  name: dirsql
  tagline: Ephemeral SQL index over a local directory
  actions:
    - theme: brand
      text: Get Started
      link: /getting-started
    - theme: alt
      text: GitHub
      link: https://github.com/thekevinscott/dirsql
---

## What is dirsql?

dirsql watches a filesystem, ingests structured files into an in-memory SQLite database, and exposes a SQL query interface. On shutdown, the database is discarded -- the filesystem remains the source of truth.

## The problem

Structured data stored as flat files (JSONL, JSON, markdown) is easy to read, write, diff, and version-control. But querying across many files is slow. "Show me all unresolved comments across 50 documents" requires opening and parsing every file.

## The solution

dirsql bridges this gap: files remain the source of truth, but you get SQL queries and real-time change events for free. Define tables with glob patterns and extract functions, and dirsql handles the rest.

```python
from dirsql import DirSQL, Table
import json

db = DirSQL(
    "./my-project",
    tables=[
        Table(
            ddl="CREATE TABLE comments (id TEXT, body TEXT, resolved INTEGER)",
            glob="comments/**/*.jsonl",
            extract=lambda path, content: [
                json.loads(line) for line in content.splitlines()
            ],
        ),
    ],
)

# SQL queries over your filesystem
unresolved = db.query("SELECT * FROM comments WHERE resolved = 0")
```

## Analogues

- **Steampipe**: SQL over cloud APIs
- **Osquery**: SQL over OS state
- **Datafusion / DuckDB**: SQL over data files (Parquet, CSV)

dirsql applies this pattern to a local project directory with real-time file watching.
