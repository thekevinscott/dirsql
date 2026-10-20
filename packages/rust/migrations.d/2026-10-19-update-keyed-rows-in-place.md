### A refresh updates a keyed row in place

#### Summary

A config table's watch refresh now matches rows by the table's `PRIMARY KEY` or a `UNIQUE` constraint the command fills, and `UPDATE`s a changed row in place instead of deleting and reinserting it. Columns the command does not emit (a generated `INTEGER PRIMARY KEY`, a default) keep their values. Nothing breaks for tables without such a key.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A keyed table's changed rows now fire `UPDATE` triggers instead of `DELETE` and `INSERT` triggers. A keyed table that keeps an FTS5 or `vec0` index current with only insert and delete triggers needs an `AFTER UPDATE` trigger as well.

#### Verification

```bash
dirsql server -c .dirsql.toml
```

With `CREATE TABLE experiments (id INTEGER PRIMARY KEY, folder_name TEXT UNIQUE, name TEXT)`, edit one experiment's `name`: the `/events` stream carries one `update`, and `SELECT id FROM experiments` returns the same ids as before.
