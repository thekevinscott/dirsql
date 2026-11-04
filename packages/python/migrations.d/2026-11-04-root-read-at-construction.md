### `DirSQL` reads its root when constructed

#### Summary

The Python `DirSQL` resolves its root (the explicit `root`, or the cwd when none is given) to an absolute path in `__init__`, not when the background scan starts. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A `chdir` between `DirSQL(...)` and the first await no longer moves the index root; the instance indexes the directory that was the cwd at construction.

#### Verification

In a directory holding `A/run.json` (`{"who": "A"}`) and `B/run.json` (`{"who": "B"}`), run `os.chdir("B")`, `db = DirSQL()`, `os.chdir("../A")`, then `await db.query("SELECT content ->> 'who' AS w FROM './run.json'")`: it prints `[{'w': 'B'}]`.
