---
name: dirsql
description: Answer a question about a directory tree by writing SQL and running it with the dirsql CLI. Use when the answer spans many files - counting, grouping, or aggregating files; "which files are ..." by size, mtime, or extension; pulling the same JSON or frontmatter field out of many files; comparing or joining two directories. Do not use to read one known file (Read), hunt for a literal string (grep), or list a handful of names (ls).
allowed-tools: Bash(uvx dirsql:*)
---

# dirsql

Learn about `dirsql`'s skill implementation with `uv`, `npm`, or `cargo`:

## `uv`

```bash
uvx dirsql context
```

## `npm`

```bash
npx -y dirsql context
```

## `cargo`
