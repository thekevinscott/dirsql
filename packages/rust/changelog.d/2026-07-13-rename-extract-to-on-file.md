**Changed**

- **Renamed the per-file row seam from `extract` to `on_file` / `onFile`** across all three SDKs, unifying on the config spelling (`on-file` in TOML). Python `Table(on_file=…)`, TypeScript `TableDef.onFile`, Rust `Table::new(ddl, glob, on_file)` and the `OnFileFn` alias (was `ExtractFn`). The Rust error variant is now `DirSqlError::OnFile` and its message reads `on-file error for {path}` (was `extract error for {path}`). Hard break — no deprecation alias. (#570)
