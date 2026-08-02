**Changed** — the wheel's compiled artifacts are now stripped and built with
fat LTO, shrinking the `_dirsql.abi3.so` extension ~28% (6.94 MB → 4.99 MB) and
the bundled CLI binary ~33% (9.17 MB → 6.14 MB).

Build-only: no API or behavior change.
