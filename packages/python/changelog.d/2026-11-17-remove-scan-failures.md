**Removed** `DirSQL.scan_failures()` and the `ScanFailure` class (no longer
importable from `dirsql`). The list has been empty in every case since a
failing per-file `on_file` callback began failing `ready()` and every query.
