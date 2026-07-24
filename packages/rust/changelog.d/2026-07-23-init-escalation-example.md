**Changed** — `dirsql init` now scaffolds an *escalation* example instead of a
catch-all `files` table. Because any path (`./`, `../`, `/`, `~/`) is already a
zero-config live view of the filesystem (`SELECT * FROM './'`), duplicating
that as a starter table taught nothing. The generated `.dirsql.toml` is now a
single named `[[table]]` (`records`) with a scoped `glob`, a pinned DDL, and a
real `on-file = "cat {path}"` hook that turns each matched `*.json` file into
rows — with a commented custom-parser stub and a header comment pointing at the
zero-config floor. The scaffold works as written: `dirsql init` then
`dirsql query "SELECT * FROM records" -c .dirsql.toml` returns the parsed rows.
The `--include-default` launcher path seeds this same table.
