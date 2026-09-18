**Added**

`ScanFailure` is now importable from the package root (`from dirsql import
ScanFailure`), so the objects `await db.scan_failures()` returns can be
annotated without reaching into `dirsql._dirsql`. The bundled type stub now
also declares `ScanFailure`, `DirSQL.scan_failures`, `run_cli`, and
`Table.on_file`, which it had been missing. No runtime behavior changes.
