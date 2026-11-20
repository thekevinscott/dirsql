### A path-table query over a folder that cannot be listed fails

#### Summary

A path-table query whose glob reaches a folder that exists but cannot be listed (its open fails, or a read fails part-way) now fails naming the folder, in every SDK and the CLI. It used to return the rows of every other folder. No signature changes.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

`query` over a path-table, such as `SELECT * FROM './data/**/*.ndjson'`, fails with `cannot list folder '<path>': <reason>` where it used to succeed. In the Rust SDK this is a SQLite module error; the CLI exits non-zero and the Python and TypeScript SDKs raise.

#### Verification

```bash
mkdir -p data/open data/locked
echo '{"k":1}' > data/open/a.ndjson
chmod 311 data/locked
dirsql "SELECT path FROM './data/**/*.ndjson'"
```

Expected (as a non-root user): exit status 1 and an error containing `cannot list folder` and `data/locked`.
