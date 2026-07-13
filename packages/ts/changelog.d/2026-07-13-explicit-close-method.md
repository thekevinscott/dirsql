**Added**

- Add explicit `close()` method to DirSQL class for cleanup and to ensure WAL checkpoint completes before external tools read the persistent cache database. (#598)
