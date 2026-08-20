### Result rows render as a table at a terminal (#989)

#### Summary

Every surface printed a JSON array. Rows now follow their **destination**: a
table when stdout is a terminal, the same JSON array when it is piped or
redirected. A new `--format {auto,table,json}` overrides that in either
direction, defaulting to `auto`.

**Piped output is unchanged, byte for byte.** `dirsql "<sql>" | jq` and
`dirsql "<sql>" > rows.json` produce exactly what they produced before.

#### Required changes

| Before | After | Fix |
| ------ | ----- | --- |
| `dirsql "<sql>"` at a terminal printed JSON you could copy | Prints a table | `dirsql "<sql>" --format json`, or redirect it — `auto` already picks JSON for any non-terminal destination. |
| A script capturing output **through a pty** (`script`, `expect`, a CI terminal emulator) parsed JSON | Sees a table | Pass `--format json` explicitly. A pty is a terminal, so `auto` cannot tell it from a human. |
| `dirsql server --format …` | Not applicable | The flag was never accepted there and still is not: the server speaks JSON over HTTP. |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Rows printed to a terminal are a table rather than a JSON array. Nothing
  changes for a pipe, a file, or any other non-terminal destination.
- The decision keys on **stdout**, not stdin. `dirsql > rows.json` typed
  interactively writes JSON; `dirsql < script.sql` with stdout on a terminal
  writes a table.
- Table cells are altered to fit a grid: newlines, tabs and other control
  characters are escaped (`\n`, `\t`, `\u{…}`), and anything longer than 60
  characters is truncated with `…`. A `content` column is the reason for both;
  use `--format json` when you need the value unaltered.
- `NULL` is spelled out in a table, so it is distinguishable from an empty
  string. JSON is unaffected — it still carries `null`.

#### Verification

```bash
# A pipe is not a terminal, so nothing changes there:
dirsql "SELECT basename FROM './'" | jq -r '.[].basename'
# a.md
# bb.md

# The same query at a terminal:
dirsql "SELECT basename, size FROM './' ORDER BY basename" --format table
# basename  size
# --------  ----
# a.md      6
# bb.md     10
#
# 2 rows

# Forcing JSON at a terminal:
dirsql "SELECT basename FROM './'" --format json
# [{"basename":"a.md"},{"basename":"bb.md"}]

# An unknown value is a usage error rather than a silent fallback:
dirsql "SELECT 1" --format yaml; echo $?
# 2
```
