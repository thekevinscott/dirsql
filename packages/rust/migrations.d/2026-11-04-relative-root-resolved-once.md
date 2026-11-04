### A relative root is resolved once, at build time

#### Summary

A relative `root` given to the builder, the Python `DirSQL`, or the TypeScript `DirSQL` is made absolute against the working directory when the index is built, and path-tables query that directory for the instance's lifetime. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

After a `chdir`, path-table queries on an instance built with a relative root return that root's files, where they returned nothing or another directory's files before. `DirSQL::root()` (and the `root` the server's frontend state reports) is the absolute path instead of the string as given.

#### Verification

In a directory holding `tree/a.json` and `tree/sub/`, build `DirSQL("tree")` from Python, run `await db.query("SELECT path FROM './**'")`, `os.chdir("tree/sub")`, and query again: both calls print `[{'path': 'a.json'}]`.
