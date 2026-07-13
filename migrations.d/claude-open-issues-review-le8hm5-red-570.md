### Rename SDK `extract` to `on_file` / `onFile`

#### Summary

The per-file → rows seam had two names for one concept: the config surface
called it `on-file` (a command), while the SDK surface called it `extract` (a
callback). This renames the SDK callback to match the config spelling on
every surface — Python `on_file`, TypeScript `onFile`, Rust `on_file` (with
the public `ExtractFn` alias becoming `OnFileFn`). It is a hard break with no
deprecation alias: the old `extract` spelling is gone in all three SDKs. The
Rust error variant and its message change too. Anyone constructing a
programmatic `Table` in Python, TypeScript, or Rust must update their call
sites; config-file (`.dirsql.toml`) users are unaffected — `on-file` was
already the config name.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Python `Table` | `Table(ddl=…, glob=…, extract=fn)` | `Table(ddl=…, glob=…, on_file=fn)` |
| TypeScript `TableDef` | `{ ddl, glob, extract: fn }` | `{ ddl, glob, onFile: fn }` |
| Rust `Table::new` / `try_new` / `strict` | `Table::new(ddl, glob, extract)` | `Table::new(ddl, glob, on_file)` |
| Rust type alias | `pub type ExtractFn` | `pub type OnFileFn` |
| Rust error variant | `DirSqlError::Extract { path, message }` | `DirSqlError::OnFile { path, message }` |

#### Deprecations removed

_None._ (There was no prior deprecation cycle; `extract` is removed directly.)

#### Behavior changes without code changes

- Rust `DirSqlError` per-file failure message: previously
  `extract error for {path}: {message}`; now `on-file error for {path}:
  {message}`. Code matching on the message text (rather than the variant)
  must update the substring. The variant itself also renamed
  (`Extract` → `OnFile`).

#### Verification

```python
from dirsql import Table
# Old spelling is now a hard error:
try:
    Table(ddl="CREATE TABLE t (n)", glob="*.json", extract=lambda p: [])
    print("FAIL: extract still accepted")
except TypeError:
    print("ok: extract rejected")
# New spelling works:
Table(ddl="CREATE TABLE t (n)", glob="*.json", on_file=lambda p: [])
print("ok: on_file accepted")
# expected:
# ok: extract rejected
# ok: on_file accepted
```
