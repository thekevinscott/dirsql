**Changed** — the `dirsql_path` and `dirsql_parsed` virtual tables now share one
piece of scaffolding instead of carrying parallel copies of it. Module-argument
decoding, glob and ignore-pattern compilation, the gitignore switch, and the
read-only table and cursor (`xBestIndex` / `xFilter` / `xNext` / `xEof` /
`xColumn` / `xRowid`) live once in an internal module; each table now supplies
only what differs — its own arguments, how it produces a row set, and how a row
becomes a cell. No public API, SQL surface, or observable behavior changes.
