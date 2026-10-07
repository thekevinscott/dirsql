**Changed**

- **The default `--persist` cache moves out of the scanned tree.** Bare `--persist` now writes `cache.db` under the platform cache directory (`$XDG_CACHE_HOME/dirsql/<root hash>/` on Linux, `~/Library/Caches` on macOS, `%LOCALAPPDATA%` on Windows) instead of `<root>/.dirsql/`. `--persist <path>` is unchanged.

**Removed**

- **The reserved `.dirsql/` directory.** The scan no longer skips a top-level `.dirsql/`; it is an ordinary dot directory, matched like bash.
