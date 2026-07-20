**Removed** the implicit no-config `files` table (#636). Building an index with
neither a config nor programmatic tables now defines no named tables;
filesystem queries are served by path-tables (`SELECT * FROM './'`).

**Added** a narrowly-scoped hint: a `no such table: files` raised in exactly
that configless state appends `did you mean FROM './'?`. A config or table set
that merely omits `files` gets the plain SQLite error.

`dirsql init` and the internal `--include-default` flag still ship the same
starter `files` table; both are explicit opt-ins, decoupled from the removed
runtime fallback.
