### `DirSQL` reads relative `extensions` paths when constructed

#### Summary

The TypeScript `DirSQL` resolves a relative programmatic `extensions` path, and the local file a bare name probes, against the cwd when `new DirSQL` is called, not when the background build starts. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A `process.chdir` between `new DirSQL(...)` and `await db.ready` no longer changes which file a relative `extensions` path loads; it loads the file under the cwd at construction.

#### Verification

In a directory holding `ext/vec0.so`, run `const db = new DirSQL({ root: ".", extensions: [{ path: "ext/vec0.so" }] })`, `process.chdir("/")`, then `await db.ready`: it resolves without a `failed to load extension` error.
