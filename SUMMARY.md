# dirsql

Ephemeral SQL index over a local directory. Watches a filesystem, ingests structured files (JSONL, JSON, markdown with frontmatter, CSV), builds an in-memory SQLite database, and exposes a SQL query interface. On shutdown, the database is discarded -- the filesystem is always the source of truth.

## Language

Rust.

## What it does

1. **Startup scan**: Walk a directory tree, parse structured files, populate SQLite tables
2. **File watching**: Monitor for changes (inotify/fswatch via `notify` crate), update the index incrementally
3. **Query interface**: Expose SQL queries over the indexed data (Unix socket, HTTP, or embedded library)
4. **Event emission**: Notify subscribers when files change (websocket or event stream)
5. **Ephemeral**: The SQLite database is in-memory or tmpfile. Discarded on shutdown. Rebuilt on next start.

## Why

The problem: structured data stored as flat files on disk (JSONL, JSON, markdown) is easy for agents and humans to read/write, git-friendly, and portable. But querying across many files is slow -- "show me all unresolved comments across 50 documents" requires opening and parsing every file.

dirsql bridges this: files remain the source of truth (readable, appendable, diffable), but you get SQL queries and change events for free.

## Motivating use case

A writing assistant app stores documents as markdown on disk with comment threads as JSONL files in a recursive workspace structure:

```
my-article/
  index.md
  _resources/
    source-1.md
  _comments/
    a1b1/
      index.jsonl        # comment thread (append-only events)
      _resources/
        deep-dive.md
      _comments/          # comments on comments
        c3d4/
          index.jsonl
```

The editor needs to:
- Query "all unresolved comments in this workspace" without scanning every file
- Get notified when an external agent appends to a thread or creates a new resource
- Remain decoupled from any specific database -- dirsql is a dev dependency, not a data store

## Analogues

- **Steampipe**: SQL over cloud APIs
- **Osquery**: SQL over OS state
- **Datafusion/DuckDB**: SQL over data files (Parquet, CSV)

dirsql is this pattern applied to a local project directory with real-time file watching.

## Open questions (for scoping conversation)

- Table schema inference: auto-detect from file structure, or require a config/schema file?
- Query interface: HTTP API, Unix socket, embedded Rust library, all three?
- Event protocol: websockets, SSE, or something simpler?
- Scope of file format support: start with JSONL only, or JSON/CSV/markdown frontmatter from day one?
- How to handle nested/recursive structures (the workspace pattern above)?
- Performance targets: how large a directory tree should startup scan handle?
