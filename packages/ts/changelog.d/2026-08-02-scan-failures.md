**Added**

`await db.scanFailures()` returns the files the initial scan could not index —
a `ScanFailure[]`, each with `path` (relative to the root) and `message` (the
hook's own error). Empty after a clean scan (#715). The `ScanFailure` type is
exported from the package root.

This closes a gap opened by #714. A file whose `onFile` hook throws, or whose
row the table rejects under `strict`, is skipped rather than failing the scan —
so a TypeScript caller whose `db.ready` previously rejected got nothing at all:
a database quietly holding fewer rows, with no way to ask which files were
dropped. `scanFailures()` is that way.

It reports; it does not gate. The rows that did land are unaffected, and a
skipped file never enters the persistent index, so it is retried on the next
scan — the index is incomplete, never wrong.

Awaits the initial scan first, so an empty array means the scan finished
cleanly rather than that it had not yet reached the failing file.
