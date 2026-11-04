### `DirSQL` reads its root when constructed

#### Summary

The TypeScript `DirSQL` resolves its root (the explicit `root`, or `process.cwd()` when none is given) to an absolute path in the constructor, not when the background scan starts. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A `process.chdir` right after `new DirSQL(...)` no longer moves the index root; the instance indexes the directory that was the cwd at construction.

#### Verification

In a directory holding `tree/a.json` and `tree/sub/`, run `const db = new DirSQL({ root: "tree" })`, `process.chdir("tree/sub")`, then `await db.query("SELECT path FROM './**'")`: it resolves to `[{ path: "a.json" }]`.
