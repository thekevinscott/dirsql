### `on-file` prints one JSON object per line

#### Summary

An `on-file` command's stdout is now NDJSON: one JSON row object per line.
The old contract, a JSON array on the last non-empty line, is rejected with a
message saying to print one object per line.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Parser script | collects rows, `print(json.dumps(rows))` | `print(json.dumps(row))` per row |
| `jq` one-liner | `jq -sc add` | `jq -c .` |
| Shell parser | `printf '[' ... ']'` with commas | one `printf '{...}\n'` per row |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Log lines on stdout are no longer ignored; send them to stderr.
- A command that exits zero and prints nothing yields a table with no rows.

#### Verification

```bash
cd "$(mktemp -d)"
printf 'alpha\n' > a.md
printf 'beta\n' > b.md
cat > rows.sh <<'SH'
for p; do printf '{"basename":"%s"}\n' "${p##*/}"; done
SH
dirsql query "SELECT basename FROM './*.md' ORDER BY basename" --on-file 'sh rows.sh'
# expected: [{"basename":"a.md"},{"basename":"b.md"}]
```
