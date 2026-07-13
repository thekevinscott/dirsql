**Changed**

- **The persistent cache (`--persist` / `persist=True` / `persist: true`) now opens `cache.db` in WAL journal mode with `synchronous=NORMAL`.** Cache rebuilds and watch updates no longer pay per-commit fsyncs; transient `cache.db-wal` / `cache.db-shm` sidecar files appear next to the cache while `dirsql` is running. (#598)
