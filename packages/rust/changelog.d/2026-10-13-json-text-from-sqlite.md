**Changed** The CLI's JSON output writes each text cell straight from
SQLite's buffer, so a query that returns many rows prints sooner. The bytes
printed are unchanged. A text cell that SQLite holds as invalid UTF-8 (for
example `CAST(x'ff' AS TEXT)`) now prints with replacement characters
instead of crashing the query.
