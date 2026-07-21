### The implicit no-config `files` table is removed (#636)

#### Summary

Constructing an index with **neither a config nor programmatic tables** no
longer injects the baked-in `files` table (added in #603). In that state
dirsql now defines **no named tables at all**; filesystem queries are served by
[path-tables](../../docs/reference/path-tables.md) — a quoted path where a
table name goes. To keep the miss discoverable, a failed `no such table: files`
raised in exactly that configless state appends `did you mean FROM './'?`. The
hint is deliberately narrow: a config or table set that merely omits `files`
gets the plain SQLite error, because there the name is a genuine typo rather
than a retired default.

`dirsql init` still writes a starter `.dirsql.toml` defining a `files` table,
and the internal `--include-default` flag still composes that same shipped
asset. Both are explicit opt-ins; neither is the implicit runtime fallback this
removes.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| CLI, no `-c` | `dirsql query "SELECT * FROM files"` | `dirsql query "SELECT * FROM './'"` |
| CLI, scoped to a glob | `dirsql query "SELECT * FROM files WHERE ext = 'md'"` | `dirsql query "SELECT * FROM './**/*.md'"` |
| SDK, no config/tables | `db.query("SELECT * FROM files")` | `db.query("SELECT * FROM './'")` |
| Want the old named table back | (implicit) | pass `-c` / `config=` with the snippet below |

To restore the retired table verbatim, write this `.dirsql.toml` (it is exactly
what `dirsql init` emits) and pass it explicitly:

```toml
[[table]]
ddl  = "CREATE TABLE files (path TEXT, basename TEXT, dir TEXT, ext TEXT, size INTEGER, mtime INTEGER, ctime INTEGER)"
glob = "**/*"
```

```bash
dirsql init                                     # writes the above
dirsql query "SELECT * FROM files" -c ./.dirsql.toml
```

#### Deprecations removed

_None._ The implicit table was never deprecated; it is removed directly.

#### Behavior changes without code changes

- With no config and no programmatic tables, `SELECT ... FROM files` now fails
  with `no such table: files; did you mean FROM './'?` instead of returning one
  row per file.
- `GET /events` emits row events only for **named** tables. With no config
  there are none, so a configless server streams `ready` and nothing else.
  Watching requires a config-defined table.
- A config or table set that defines tables but not `files` is unchanged: it
  still gets the plain `no such table: files`, with no hint.
- `dirsql init` and `--include-default` are unchanged.

#### Verification

```bash
cd "$(mktemp -d)" && touch a.md
dirsql query "SELECT * FROM files"
# error: no such table: files; did you mean FROM './'?
dirsql query "SELECT basename FROM './'"
# [{"basename":"a.md"}]
```
