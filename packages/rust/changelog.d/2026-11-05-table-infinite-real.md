**Fixed** `--format table` (one-shot and REPL) prints an infinite REAL as
`inf` / `-inf` instead of `NULL`, which it reserves for SQL NULL. JSON output
still maps a non-finite REAL to `null`.
