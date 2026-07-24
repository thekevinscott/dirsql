**Removed**

- **Removed the stat-fact injection layer** (core change, surfaced through the Python SDK). dirsql no longer auto-fills DDL columns named `path`/`basename`/`dir`/`ext`/`size`/`mtime`/`ctime` from filesystem stat. A `Table` now contains exactly what its `on_file` callback returns; a DDL column the callback never emits is `None`/`NULL`, not injected. To keep a stat column populated, emit it from the callback (e.g. derive `path` from the file argument), or query the path directly with a path-table (`FROM './'`). (#634)
