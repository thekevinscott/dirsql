### `scan_failures` is removed

#### Summary

TypeScript `DirSQL.scanFailures()` and the `ScanFailure` type are removed. The list was always empty: a per-file `onFile` callback that throws, or returns a row the table rejects, makes `ready` and every query reject. This breaks code that calls the method or imports the type.

#### Required changes

| Before | After |
|---|---|
| `await db.scanFailures()` | Delete the call; catch the `Error` that `await db.ready` and `await db.query(...)` reject with |
| `import type { ScanFailure } from "dirsql"` | Delete the import |

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```sh
node --input-type=module -e "import { DirSQL } from 'dirsql'; console.log(typeof DirSQL.prototype.scanFailures)"
```

prints `undefined`.
