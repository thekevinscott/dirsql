### Core: the path tables of one statement are walked at the same time

#### Summary

A statement that names several path tables used to walk their trees one
after the other, each walk running inside SQLite's first `xFilter` on that
table's cursor. The walks now start together when the statement is prepared,
each on a thread of its own, and the first cursor over a table joins its
walk. Nothing in the API changed: no signature, flag, config key, exit code
or row shape differs. Rust callers, the CLI and the Python and TypeScript
SDKs all pick this up without edits.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Every path table a statement reads is walked as soon as the statement is
  prepared, even one whose cursor SQLite ends up never opening (the inner
  side of a join whose outer side turns out empty). Before, such a table cost
  nothing; now it costs a walk on a background thread. The rows are the same
  either way.
- A statement over several path tables uses as many threads as it names
  tables while they walk, so it runs on more than one core at once.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir -p papers/one papers/two
printf 'T1' > papers/one/title.md
printf 'A1' > papers/one/abstract.md
printf 'T2' > papers/two/title.md
dirsql query "SELECT a.dir, t.content AS title FROM './papers/*/abstract.md' a JOIN './papers/*/title.md' t ON t.dir = a.dir ORDER BY a.dir"
# expected: [{"dir":"papers/one","title":"T1"}] -- the same rows as before, one walk's worth of wall time
```
