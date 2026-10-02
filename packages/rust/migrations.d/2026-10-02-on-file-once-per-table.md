### Core: `on-file` runs once per table with the matched paths as trailing arguments

#### Summary

An `on-file` command no longer runs once per matched file with `{path}`
substituted. It runs once per table, every matched path appended to its
arguments in scan order, and must print one JSON array of row objects for the
whole table. Every surface that names an `on-file` command is affected: the
`--on-file` flag, the `on-file` key of a `[[table]]` entry, and the parser
script those name. A command still written with `{path}` is rejected at
startup.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `--on-file` flag | `--on-file 'python3 extract.py {path}'` | `--on-file 'python3 extract.py'` |
| `[[table]]` `on-file` key | `on-file = "python3 extract.py {path}"` | `on-file = "python3 extract.py"` |
| Parser script | reads `sys.argv[1]`, prints one array per file | iterates `sys.argv[1:]`, prints one array |
| `jq` one-liner | `jq -c '[.]' {path}` | `jq -c -s '.'` or `jq -sc add` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A table's command runs once, not once per file; a table over no files runs
  nothing.
- A failing command fails its whole table. The CLI names the table where it
  named the file (`dirsql: skipping `notes`: ...`), exits 23 as before, and
  the table queries as empty.
- Watch mode re-runs the whole table on any change under its glob; rows are
  replaced rather than updated per file.
- `OnFileFailure.path` carries the table's name when the command failed over
  the whole table.

#### Verification

```bash
cd "$(mktemp -d)"
printf 'alpha\n' > a.md
printf 'beta\n' > b.md
cat > rows.sh <<'SH'
printf '['; sep=""
for p; do printf '%s{"basename":"%s"}' "$sep" "${p##*/}"; sep=","; done
printf ']'
SH
dirsql query "SELECT basename FROM './*.md' ORDER BY basename" --on-file 'sh rows.sh'
# expected: [{"basename":"a.md"},{"basename":"b.md"}]
dirsql query "SELECT 1" --on-file 'sh rows.sh {path}'
# expected on stderr: on-file command `sh rows.sh {path}` uses `{path}`, but an
# on-file command now runs once per table with every matched path appended as
# trailing arguments and prints one JSON array of row objects.
```
