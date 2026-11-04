**Fixed**

- **A hook key fills its declared column whatever its ASCII case.** A key `title` emitted for a column declared `TITLE`, by an `on-file` command or an SDK `Table` closure, now stores its value instead of a silent `NULL`, and a `strict` table accepts it instead of reporting an extra and a missing column. Keys and columns compare as SQLite compares identifiers: case-insensitively for ASCII letters only.
