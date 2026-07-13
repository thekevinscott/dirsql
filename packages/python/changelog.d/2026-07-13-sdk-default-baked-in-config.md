**Changed**

- **`DirSQL(root)` with no `config` and no `tables` now serves the baked-in default `files` table**, instead of an empty index — parity with the CLI's no-`-c` default (#603). `await db.query("SELECT * FROM files")` works out of the box. Passing `config=` (a path or list of paths) or programmatic `tables` is unchanged; there is still no implicit `<root>/.dirsql.toml` discovery. The behavior lives in the shared Rust core builder; the Python SDK signature is unchanged.
