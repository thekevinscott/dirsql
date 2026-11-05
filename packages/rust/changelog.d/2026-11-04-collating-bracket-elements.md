**Fixed**

- **Collating symbols and equivalence classes in a glob bracket.** `[[.a.]]`, a POSIX-named symbol such as `[[.hyphen.]]`, and `[[=a=]]` now match their character as bash does, on path-tables and config globs alike; a collating symbol may bound a range (`[[.a.]-c]`). They previously matched nothing.
