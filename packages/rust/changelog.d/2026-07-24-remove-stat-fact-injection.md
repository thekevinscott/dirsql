**Removed**

- **Removed the stat-fact injection layer.** dirsql no longer auto-fills DDL columns named `path`/`basename`/`dir`/`ext`/`size`/`mtime`/`ctime` from filesystem stat. A named table now contains exactly what its `on-file` hook emits; a DDL column the hook never emits is `NULL` (standard SQL), not injected. To keep a stat column populated, emit it from the hook (e.g. derive `path` from the file argument), or query the path directly with a path-table (`FROM './'`). (#634)
