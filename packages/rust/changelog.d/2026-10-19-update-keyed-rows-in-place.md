**Fixed**

- **A refresh updates a changed row in place.** When a config table's `on-file` command re-runs in watch mode, rows are matched by the table's `PRIMARY KEY`, or else a `UNIQUE` constraint the command fills, and a changed row is `UPDATE`d with its rowid kept, emitting an `update` event. Columns the command does not emit, such as a generated `INTEGER PRIMARY KEY`, are never compared or overwritten, so an unchanged refresh writes nothing and ids stay put. A table with no such key still deletes and inserts the rows that changed.
