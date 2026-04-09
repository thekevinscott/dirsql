# Getting Started

## Installation

```bash
pip install dirsql
```

## Quick start

Suppose you have a directory of JSON files representing blog posts:

```
my-blog/
  posts/
    hello.json      # {"title": "Hello World", "author": "alice"}
    second.json     # {"title": "Second Post", "author": "bob"}
  authors/
    alice.json      # {"id": "alice", "name": "Alice"}
    bob.json        # {"id": "bob", "name": "Bob"}
```

Index and query them with dirsql:

```python
from dirsql import DirSQL, Table
import json

db = DirSQL(
    "./my-blog",
    tables=[
        Table(
            ddl="CREATE TABLE posts (title TEXT, author TEXT)",
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

# Query all posts
posts = db.query("SELECT * FROM posts")
# [{"title": "Hello World", "author": "alice"}, {"title": "Second Post", "author": "bob"}]

# Join across tables
results = db.query("""
    SELECT posts.title, authors.name
    FROM posts
    JOIN authors ON posts.author = authors.id
""")
# [{"title": "Hello World", "name": "Alice"}, {"title": "Second Post", "name": "Bob"}]
```

## What happens at startup

1. dirsql walks the directory tree
2. Files matching each table's glob pattern are read
3. The `extract` function converts file content into rows
4. Rows are inserted into an in-memory SQLite database
5. SQL queries run against that database

The database is ephemeral. It exists only while the `DirSQL` instance is alive. The filesystem is always the source of truth.

## Next steps

- [Defining Tables](./guide/tables.md) -- DDL, globs, and extract functions in detail
- [Querying](./guide/querying.md) -- SQL queries and return format
- [File Watching](./guide/watching.md) -- real-time change events
- [Async API](./guide/async.md) -- non-blocking usage with AsyncDirSQL
