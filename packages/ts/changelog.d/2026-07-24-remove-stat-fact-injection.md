**Removed**

- **Removed the stat-fact injection layer** (core change, surfaced through the TypeScript SDK). dirsql no longer auto-fills DDL columns named `path`/`basename`/`dir`/`ext`/`size`/`mtime`/`ctime` from filesystem stat. A `TableDef` now contains exactly what its `onFile` callback returns; a DDL column the callback never emits is `null`/`NULL`, not injected. To keep a stat column populated, emit it from the callback (e.g. derive `path` from the file argument), or query the path directly with a path-table (`FROM './'`). (#634)
