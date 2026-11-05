**Fixed** A file whose name is not valid Unicode keeps its own bytes in a
path-table's `path`, `basename`, `dir`, and `ext`, instead of a replacement
character that made two such files share one path naming neither. Reading
such a column as text fails with
`column "path" holds text that is not valid UTF-8`; `hex(path)` and
`CAST(path AS BLOB)` give the exact name, and `WHERE path = CAST(x'..' AS TEXT)`
finds the file. A table's `on-file` command now receives the name as it is on
disk, not a replacement-character path that does not exist.
