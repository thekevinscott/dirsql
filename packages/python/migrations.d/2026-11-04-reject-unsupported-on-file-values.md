### `on_file` refuses unsupported value types

#### Summary

The Python SDK's `on_file` rows accept only `str`, `int`/`bool`, `float`, `bytes`/`bytearray` and `None`. Any other value type now raises `TypeError` (the file is skipped and reported in `scan_failures()`) instead of being coerced to `REAL` through `__float__` or stored as its repr. Hooks that returned such values must convert them.

#### Required changes

| Before | After |
|---|---|
| `{"n": decimal.Decimal("1.5")}` | `{"n": float(d)}` or `{"n": str(d)}` |
| `{"n": numpy.int64(7)}` | `{"n": int(x)}` |
| `{"meta": {"a": 1}}` | `{"meta": json.dumps(m)}` |
| `{"tags": ["a", "b"]}` | `{"tags": json.dumps(tags)}` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

A hook returning an unsupported type no longer indexes that file; `scan_failures()` lists it with a `TypeError` naming the column and type.

#### Verification

```sh
python -c 'import asyncio, decimal, pathlib, tempfile; from dirsql import DirSQL, Table
d = tempfile.mkdtemp(); pathlib.Path(d, "a.json").write_text("{}")
async def main():
    db = DirSQL(d, tables=[Table(name="t", ddl="CREATE TABLE t (v)", glob="*.json", on_file=lambda p: [{"v": decimal.Decimal(1)}])])
    print([f.message for f in await db.scan_failures()])
asyncio.run(main())'
```

Expected: one message containing `TypeError: column 'v': unsupported value type Decimal`.
