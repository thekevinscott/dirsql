### `dirsql init` output rewritten as an escalation example (#637)

#### Summary

`dirsql init` no longer writes a catch-all `files` table over `**/*` with a
stat-column schema. That table only duplicated the zero-config path-table floor
(`SELECT * FROM './'`), which already lists every file with `path`, `basename`,
`dir`, `ext`, `size`, `mtime` and `ctime` and no config at all. The generated
`.dirsql.toml` is now an **escalation** scaffold: one named `[[table]]`
(`records`) with a scoped `glob = "**/*.json"`, a pinned
`ddl = "CREATE TABLE records (id TEXT, name TEXT)"`, and a real
`on-file = "cat {path}"` hook whose rows come entirely from the hook (dirsql
injects no columns). The internal `--include-default` launcher path seeds this
same table's glob and DDL.

The public `init` contract is otherwise unchanged: `--root`, `--output`,
`--force`, the no-auto-load behavior, and every exit-code / error arm are the
same. Only the *bytes written* changed.

Carrying a genuine `on-file` hook keeps the shipped asset a valid config both
today and once hook-less `[[table]]` entries become a load error, so
`--include-default` (which parses the asset with `.expect(...)`) never panics.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Query the init scaffold | `dirsql query "SELECT * FROM files" -c .dirsql.toml` | `dirsql query "SELECT * FROM records" -c .dirsql.toml` |
| Want the old `files` table | (what `init` emitted) | write it yourself (snippet below) and pass with `-c` |

To restore the previous starter table verbatim, write this `.dirsql.toml` and
pass it explicitly:

```toml
[[table]]
ddl  = "CREATE TABLE files (path TEXT, basename TEXT, dir TEXT, ext TEXT, size INTEGER, mtime INTEGER, ctime INTEGER)"
glob = "**/*"
```

Note: this hook-less form loads today but is on track to become a config error;
prefer the zero-config `SELECT * FROM './'` for filesystem listings.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `dirsql init` writes different bytes: a `records` escalation example rather
  than a `files` catch-all table. No flags or exit codes changed.
- `--include-default` now seeds a `records` table (glob `**/*.json`) instead of
  a `files` table (glob `**/*`).

#### Verification

```bash
cd "$(mktemp -d)"
printf '[{"id":"1","name":"widget"}]' > things.json
dirsql init
dirsql query "SELECT id, name FROM records" -c .dirsql.toml
# [{"id":"1","name":"widget"}]
dirsql query "SELECT basename FROM './'"    # the zero-config floor still needs no config
# [{"basename":".dirsql.toml"},{"basename":"things.json"}]
```
