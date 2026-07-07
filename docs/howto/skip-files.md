# Skip files you don't want indexed

Drafts, build output, and editor droppings don't belong in your tables.
[`ignore`](../reference/config.md#dirsql-keys) globs exclude files entirely —
from the initial scan and from watch events alike.

## 1. Add `ignore` patterns

Suppose finished notes live in `notes/`, but drafts and scratch files hide
among them:

```
notes/final.md
notes/drafts/wip.md
notes/scratch.md.tmp
```

Exclude the noise in `.dirsql.toml`:

```toml
[dirsql]
ignore = ["notes/drafts/**", "**/*.tmp"]

[[table]]
ddl  = "CREATE TABLE notes (_path TEXT)"
glob = "notes/**/*"
```

Patterns match against root-relative paths, the same way table globs do. An
ignored file never reaches any table — even one whose glob would match it.

## 2. Confirm what made it in

```bash
dirsql query "SELECT _path FROM notes ORDER BY _path"
```

```json
[{"_path":"notes/final.md"}]
```

## Notes

- `ignore` lives in a config file, so it needs one:
  [zero-config mode](../reference/cli.md#zero-config-mode) indexes
  everything with no ignores.
- The top-level `.dirsql/` directory is always excluded, ignore list or
  not — it is reserved for `dirsql`'s own metadata
  ([config reference](../reference/config.md#dirsql-keys)).
- Narrow table globs are the other half of the story: a file matching no
  table's glob contributes no rows either. Use `ignore` for things that
  should never be looked at; use precise globs to shape what each table
  sees ([Define tables for your files](./define-tables.md)).
- Embedding `dirsql` instead? The SDK constructor takes the same patterns
  via its `ignore` parameter
  ([SDK reference](../reference/sdk.md#constructor)).
