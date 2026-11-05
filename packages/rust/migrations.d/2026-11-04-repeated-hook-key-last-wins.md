### A repeated hook key keeps its last value

#### Summary

When one line of a hook's NDJSON output repeats a key, every table now keeps that key's last value, as `jq` and `JSON.parse` do. A path-table read through `--on-file` or `path_table_parser` used to keep the first. Output with no repeated keys is unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

For a row `{"k":"first","k":"second"}`, `dirsql query "SELECT k FROM './d.ndjson'" --on-file cat` returned `first`; it now returns `second`, matching a `[[table]]` over the same command.

#### Verification

`printf '%s\n' '{"k":"first","k":"second"}' > d.ndjson` then `dirsql query "SELECT k FROM './d.ndjson'" --on-file cat --format json` prints `[{"k":"second"}]`.
