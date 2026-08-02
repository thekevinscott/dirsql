**Added**

`await db.scan_failures()` returns the files the initial scan could not index —
a list of `ScanFailure`, each with `path` (relative to the root) and `message`
(the hook's own error). Empty after a clean scan (#715).

This closes a gap opened by #714. A file whose `on_file` hook raises, or whose
row the table rejects under `strict`, is skipped rather than failing the scan —
so a Python caller who previously got an exception got nothing at all: a
database quietly holding fewer rows, with no way to ask which files were
dropped. `scan_failures()` is that way.

It reports; it does not gate. The rows that did land are unaffected, and a
skipped file never enters the persistent index, so it is retried on the next
scan — the index is incomplete, never wrong.

Awaits `ready()` first, so an empty list means the scan finished cleanly rather
than that it had not yet reached the failing file.
