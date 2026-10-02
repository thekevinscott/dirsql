### Core: a failed `on-file` command fails the build; no exit code 23, no parsed-row cache

#### Summary

An `on-file` command that fails is no longer reported as a skipped table
while the run continues. Its table cannot be filled, so the build fails with
an error naming the table and carrying the command's stderr tail. The CLI's
partial-scan exit code `23` and the `dirsql: skipping ...` stderr lines are
gone, and a path-table parsed with `--on-file` is no longer cached under
`--persist`.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Shell script checking `dirsql query`'s exit code | treats `23` as "rows on stdout, some tables empty" | a failed command exits `1` with nothing on stdout; `0` means every table filled |
| SDK caller reading `scan_failures()` / `scanFailures()` for a config-declared table | the failed table appears under its name | the constructor returns `DirSqlError::TableCommand` (Python `RuntimeError`, TypeScript `Error`) instead |
| `--persist` with `--on-file` | second run over an unchanged tree served the parsed rows from the cache | the parser runs on every start |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A `[[table]]` whose command fails now fails the whole build, including every
  other table; previously the other tables were queryable and the failed one
  was empty.
- `OnFileFailure.path` is always a file path again; it never carries a table
  name.
- An existing persist cache keeps its `_dirsql_parsed_rows` table, now unread.
  Delete the cache file to reclaim the space; the next start rebuilds it.

#### Verification

```bash
cd "$(mktemp -d)"
printf 'alpha\n' > a.md
cat > .dirsql.toml <<'TOML'
[[table]]
name = "notes"
ddl = "CREATE TABLE notes (name TEXT)"
glob = "*.md"
on-file = "sh -c 'echo nope >&2; exit 1'"
TOML
dirsql query "SELECT name FROM notes" -c .dirsql.toml
echo "exit $?"
# expected on stderr: dirsql query: ... table `notes`: ... nope
# expected: exit 1
```
