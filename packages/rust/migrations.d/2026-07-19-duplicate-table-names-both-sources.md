### `DirSqlError::DuplicateTable` becomes a struct variant naming both sources

#### Summary

A duplicate table name has always been rejected at registration, but the error
named only the table: `duplicate table name: notes`. A user whose programmatic
`Table` collided with a config-defined one had no pointer to either definition.
`DirSqlError::DuplicateTable` now carries where each side came from, so the
message names both. This breaks Rust callers that pattern-match the variant, and
changes the message text every SDK surfaces (Python, TypeScript, the CLI, and
the HTTP API all render the core's error string).

#### Required changes

| Surface | Before | After |
|---|---|---|
| Rust pattern match | `Err(DirSqlError::DuplicateTable(name))` | `Err(DirSqlError::DuplicateTable { name, first, second })` |
| Rust construction | `DirSqlError::DuplicateTable(name)` | `DirSqlError::DuplicateTable { name, first: TableSource::Programmatic, second: TableSource::Programmatic }` |
| Error message (mixed origins) | `duplicate table name: notes` | `Table 'notes' is defined by both a programmatic table and config /proj/dirsql.toml` |
| Error message (same origin) | `duplicate table name: notes` | `Table 'notes' is defined twice by config /proj/dirsql.toml` |

Rust callers that only need the name can bind it alone and ignore the rest:

```rust
Err(DirSqlError::DuplicateTable { name, .. }) => eprintln!("collision on {name}"),
```

The new `TableSource` enum is exported from the crate root:

```rust
pub enum TableSource {
    Programmatic,      // "a programmatic table"    -- added via .table() / .tables()
    Config(PathBuf),   // "config <path>"           -- a [[table]] entry in that file
    Default,           // "the built-in default config"
}
```

Python and TypeScript callers need no code change: the exception/`Error` type is
unchanged and only the message text differs. Code that matched on the **string**
`"duplicate table name"` must match the new wording (or, better, stop matching on
message text).

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **Which collisions are detected is unchanged** — the same registrations failed
  before and fail now. Only the diagnostic improved.
- **The `--include-default` collision now blames the default, not user code.**
  `--include-default` seeds the baked-in `files` table through the builder, so it
  previously reported as "a programmatic table"; it now reports as "the built-in
  default config", which is what the user actually asked for.
- A collision whose two sides share an origin (two `[[table]]` entries in one
  config, two programmatic tables) reads `defined twice by <source>` rather than
  naming the same source twice.

#### Verification

With a config that defines the same table twice:

```bash
cat > /tmp/dup.toml <<'TOML'
[[table]]
ddl = "CREATE TABLE notes (path TEXT)"
glob = "**/*.md"

[[table]]
ddl = "CREATE TABLE notes (basename TEXT)"
glob = "**/*.txt"
TOML

dirsql query "SELECT * FROM notes" -c /tmp/dup.toml; echo "exit=$?"
```

Expected:

```
dirsql query: failed to load config: Table 'notes' is defined twice by config /tmp/dup.toml
exit=1
```

Before this change the same command printed
`dirsql query: failed to load config: duplicate table name: notes`.
