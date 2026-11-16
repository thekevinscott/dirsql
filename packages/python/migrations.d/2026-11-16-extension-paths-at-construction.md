### `DirSQL` reads relative `extensions` paths when constructed

#### Summary

The Python `DirSQL` resolves a relative programmatic `extensions` path, and the local file a bare name probes, against the cwd at `__init__`, not when the background build starts. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A `chdir` between `DirSQL(...)` and the first await no longer changes which file a relative `extensions` path loads; it loads the file under the cwd at construction.

#### Verification

In a directory holding `ext/vec0.so`, run `db = DirSQL(".", extensions=[{"path": "ext/vec0.so"}])`, `os.chdir("/")`, then `await db.ready()`: it completes without a `failed to load extension` error.
