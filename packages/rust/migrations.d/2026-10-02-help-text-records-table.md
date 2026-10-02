### `dirsql --help` describes the starter table and output format accurately

**Summary**

The top-level `--help`, `init --help`, and `query --help` text now says the
starter `.dirsql.toml` defines a `records` table over `**/*.json` (it never
defined a `files` table) and that query rows print as a table on a terminal
and a JSON array when piped. Help text only: no flag, signature, exit code, or
output changed.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

_None._ The wording of `--help` differs; a script matching the old sentence
"defining a `files` table" or "prints the result rows as JSON" against
`dirsql --help` output no longer matches.

**Verification**

```bash
dirsql --help | grep -c 'a `records` table over `\*\*/\*.json`'
# 1
```
