### Hook-less `[[table]]` is now a config error (#634)

#### Summary

A `[[table]]` (or SDK-registered table) with no `on-file` hook previously
loaded fine and — after the stat-fact injection layer was removed — produced
rows that were all-NULL. dirsql now rejects a hook-less table at config load
(`ConfigError::HooklessTable`), so the failure is loud and actionable instead
of a silently useless table. No API signature changes — this is a
load-time validation change. `TableConfig.on_file` is now `String` (was
`Option<String>`) since a parsed table always carries a hook.

#### Required changes

| Before (loaded, all-NULL rows) | After (load error) | Fix |
| ------------------------------ | ------------------ | --- |
| `[[table]]` declares `ddl` + `glob` but no `on-file` | Load fails: `[[table]] '<glob>' has no on-file hook …` | Add an `on-file` hook that emits the declared columns, e.g. `on-file = 'rel=${1#"$2"/}; jq -n --arg path "$rel" '\''[{path:$path}]'\'''`. |
| You only wanted stat columns and wrote no hook code | Load fails | Drop the `[[table]]` and query the path directly with a path-table: `SELECT path, size, mtime FROM './'` (globbable: `FROM './**/*.md'`). |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A `[[table]]` with no `on-file` hook, which used to load (and, after injection
  removal, yield all-NULL rows), now fails at config load with an error naming
  the offending glob and pointing at the `FROM './'` path-table replacement.

#### Verification

```bash
# .dirsql.toml with a hook-less table:
#   [[table]]
#   ddl  = "CREATE TABLE files (path TEXT, size INTEGER)"
#   glob = "**/*.md"

# Before this change:
dirsql query "SELECT * FROM files" -c ./.dirsql.toml
# -> [] or all-NULL rows (loaded, but useless)

# After this change:
dirsql query "SELECT * FROM files" -c ./.dirsql.toml
# -> error: [[table]] '**/*.md' has no on-file hook, so every row would be
#    all-NULL. Add an `on-file` hook that emits the columns, or, for stat
#    columns with no code, query the path directly: `FROM './'`

# Replacement with no hook code — query the path directly:
dirsql query "SELECT path, size FROM './**/*.md'"
# -> rows with real path/size, no config needed
```
