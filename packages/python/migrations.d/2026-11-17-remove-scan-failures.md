### `scan_failures` is removed

#### Summary

Python `DirSQL.scan_failures()` and the `ScanFailure` class are removed. The list was always empty: a per-file `on_file` callback that raises, or returns a row the table rejects, makes `ready()` and every query raise. This breaks code that calls the method or imports the class.

#### Required changes

| Before | After |
|---|---|
| `await db.scan_failures()` | Delete the call; catch the `RuntimeError` that `await db.ready()` and `await db.query(...)` raise |
| `from dirsql import ScanFailure` | Delete the import |

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```sh
python -c "import dirsql; print(hasattr(dirsql, 'ScanFailure'), hasattr(dirsql.DirSQL, 'scan_failures'))"
```

prints `False False`.
