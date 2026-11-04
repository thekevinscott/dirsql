**Fixed** A file longer than SQLite's string length limit (1e9 bytes by
default) now reads as `NULL` `content`, as an unreadable file does, instead of
failing the whole query with `string or blob too big`. Such a file is no longer
read into memory: one whose stat size is over the limit is never opened, and
one that has grown past it since is read no further than one byte past it.
