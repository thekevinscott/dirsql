**Changed** A foreign-key violation names every file holding an offending row,
not only those among the first ten rows it lists: past the limit the message
reads ``(and N more, from `<file>`, table `<table>`)``, a table standing for rows
no single file owns. `foreign_key::ForeignKeyViolation` replaces its `table`
and `file` fields (the first offender) with `offenders: Vec<Offender>`, every
distinct table and file pair.
