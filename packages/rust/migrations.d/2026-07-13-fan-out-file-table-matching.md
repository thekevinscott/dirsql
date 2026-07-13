### A file matching multiple tables' globs now populates every matching table (#580)

#### Summary

`dirsql` previously routed a file that matched several tables' globs to only
the **first-declared** matching table; every other matching table received
zero rows for that file. A file matching N tables' globs now populates **all
N** tables — each `Table` is an independent view over the files matching its
glob. This affects every SDK (Python, TypeScript, Rust) and the CLI, since the
behavior lives in the shared core; no public API signature changed. The most
common way to be bitten is a catch-all table (e.g. glob `**/*`) that used to
ingest only the files no earlier table claimed and now ingests **all** matching
files.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Catch-all table overlapping a narrower one (any SDK / `.dirsql.toml`) | The catch-all silently received only files unclaimed by earlier tables | The catch-all receives **all** files its glob matches, including those also claimed by other tables. Tighten the catch-all's glob, or filter unwanted files in its `on-file`, to restore the previous row set. |
| Two tables intentionally sharing files | Only the first-declared table was populated | Both tables are populated; no change needed if that is what you wanted. |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **File→table routing**: a file matching N tables' globs previously produced
  rows in only the first-declared matching table; now it produces rows in every
  matching table. Declaration order no longer affects which tables a file
  populates (it may still affect event ordering within a single file event,
  which is unspecified).
- **Glob captures**: each matching table's rows now receive the captures from
  **that table's own glob**. Previously captures were resolved by a separate
  first-match lookup, so an overlapping table could receive another table's
  captures (or none).
- **Watch deletes**: deleting a file now removes its rows from **every**
  matching table and emits a `Delete` event for each, rather than only the
  first-declared table.
- **Persistent cache**: the sidecar `_dirsql_files` bookkeeping is now keyed by
  `(rel_path, table_name)` instead of `rel_path` alone, and the sidecar schema
  version is bumped (`3` → `4`). A cache written by an older build is discarded
  and rebuilt once, automatically, on the first run after upgrading — no action
  required, and penalty-free per the persistence design.

#### Verification

With a `.dirsql.toml` declaring two tables whose globs both match the same
file:

```toml
[[table]]
ddl = "CREATE TABLE ta (path TEXT)"
glob = "data/*/metadata.json"

[[table]]
ddl = "CREATE TABLE tb (path TEXT)"
glob = "data/**/metadata.json"
```

and a single file `data/2401.00001/metadata.json`, both tables are populated:

```sh
dirsql query "SELECT path FROM ta"
# expected: [{"path":"data/2401.00001/metadata.json"}]
dirsql query "SELECT path FROM tb"
# expected: [{"path":"data/2401.00001/metadata.json"}]
```

Before this change, `tb` (declared second) would have returned `[]`.
