**Fixed** A file modified (or created) before 1970 now reports `mtime` (and
`ctime`) as negative Unix seconds, floored like `stat -c %Y`, instead of NULL,
so `WHERE mtime < ...` no longer drops the oldest files.
