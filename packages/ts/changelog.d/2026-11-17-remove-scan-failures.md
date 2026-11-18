**Removed** `DirSQL.scanFailures()` and the `ScanFailure` type (no longer
exported from the package). The list has been empty in every case since a
failing per-file `onFile` callback began failing `ready` and every query.
