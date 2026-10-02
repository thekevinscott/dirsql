**Changed** An `on-file` command runs once per table, with every matched
path appended to its arguments in scan order, and must print one JSON array
of row objects. One run over a thousand files replaces a thousand interpreter
startups. A table whose paths outgrow one argument list is split into the
fewest runs that fit and their arrays joined; the command cannot tell. No
matched paths spawns nothing. A non-zero exit, empty output, or output that
is not an array of objects fails the whole table with the stderr tail; there
is no per-file skipping. In watch mode a change under a table's glob re-runs
the command over all of the table's paths and replaces its rows. A command
containing `{path}` is rejected at startup. Applies to the `--on-file` path
table and to `[[table]]` entries alike. Rows reach SQLite through one
prepared statement per table instead of one per row.
