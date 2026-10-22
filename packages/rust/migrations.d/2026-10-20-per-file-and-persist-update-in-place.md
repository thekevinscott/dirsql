### Per-file hooks and the persist refill update a keyed row in place

#### Summary

A per-file SDK hook's file change and the `--persist` warm-start refill of a config table now match rows by the table's `PRIMARY KEY` or a `UNIQUE` constraint the rows fill, and `UPDATE` a changed row in place instead of deleting and reinserting it. Columns the hook does not emit (a generated `INTEGER PRIMARY KEY`, a default) keep their values. Nothing breaks for tables without such a key.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A keyed table's changed rows now fire `UPDATE` triggers instead of `DELETE` and `INSERT` triggers on these paths too, and an edited parent row no longer fires `ON DELETE CASCADE`. A keyed table that keeps an FTS5 or `vec0` index current with only insert and delete triggers needs an `AFTER UPDATE` trigger as well.

#### Verification

```bash
dirsql query "SELECT id, folder_name FROM experiments" -c .dirsql.toml --persist
```

With `CREATE TABLE experiments (id INTEGER PRIMARY KEY, folder_name TEXT UNIQUE, name TEXT)`, run it, add a new experiment file that sorts first, and run it again: every existing `folder_name` keeps its `id`.
