### Config globs anchor at the config file's directory (#1232)

#### Summary

A `[[table]]` `glob` is matched beneath the directory of the config file that wrote it (or, for a `/`, `~/` or `../` glob, beneath its literal prefix), no longer beneath the index root. A config that relied on running from the directory it describes still works when that is also the config's directory; a config kept apart from its data, or one given an explicit root, now indexes the config's directory and needs its globs rewritten.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `DirSQL::builder().root("/data").config("/cfg/.dirsql.toml")` with `glob = "**/*.md"` | indexes `/data` | indexes `/cfg`; write `glob = "/data/**/*.md"` to index `/data` |
| `dirsql -c /cfg/.dirsql.toml` run from `/data` | indexes `/data` | indexes `/cfg` |
| `ignore` in a config | applied to every table and to path-tables | applies to that config's tables, relative to their anchor |
| A table over a directory other than the config's | `glob = "docs/*.md"` with the root elsewhere | `glob = "/data/docs/*.md"` or `glob = "~/data/docs/*.md"` |
| `{root}` in an `on-file` command | the index root | the table's anchor (the config's directory, or the literal prefix of an absolute glob) |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A config table's `path` column is relative to its anchor.
- A programmatic table and a path-table still use the index root.

#### Verification

```bash
mkdir -p /tmp/cfg/projects/p /tmp/elsewhere
echo '{}' > /tmp/cfg/projects/p/a.jsonl
printf '[[table]]\nname = "sessions"\nddl = "CREATE TABLE sessions (n TEXT)"\nglob = "projects/*/*.jsonl"\non-file = "echo [{}]"\n' > /tmp/cfg/.dirsql.toml
cd /tmp/elsewhere && dirsql query "SELECT COUNT(*) AS n FROM sessions" -c /tmp/cfg/.dirsql.toml   # [{"n":1}]
```
