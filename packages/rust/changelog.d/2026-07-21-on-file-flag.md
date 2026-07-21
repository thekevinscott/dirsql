**Added**

`dirsql query "<sql>" --on-file '<command>'` attaches a parser to every
[path-table](https://thekevinscott.github.io/dirsql/reference/path-tables) in
the query. With the flag set, a path-table's rows and schema come from the
command's JSON output (the `on-file` hook contract — argv splitting,
`{path}`/`{root}` placeholders, timeout) instead of the stat columns. The
parser's output is the whole schema; stat columns are not reachable on a parsed
path-table. Failures are isolated per file — a file whose parser fails or whose
output does not parse contributes no rows and a stderr warning, and the scan
continues. Parsed scans honor the same `node_modules`/`.git`/`ignore` skip
rules stat scans do. The flag may be given at most once (a repeat errors,
pointing at config files), applies only to path-tables (config `[[table]]`
`on-file` hooks are untouched), and is `query`-only (server mode rejects it).
