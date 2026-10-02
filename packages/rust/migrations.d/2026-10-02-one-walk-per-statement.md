### Core: `vtab::load_module` and `parsed_vtab::load_module` take a `StatementScope`

#### Summary

A `dirsql_path` or `dirsql_parsed` table registered on a raw `rusqlite`
connection now needs a `StatementScope`: the object that ends a statement, so
the rows a table cached for one statement are dropped before the next. Both
`load_module` functions gain an `Arc<StatementScope>` argument, and a caller
that runs its own statements against those tables must `reset()` the scope
(or hold a guard from `enter()`) around each one. Only Rust callers that
register the modules by hand are affected; `Db`, the CLI and the Python and
TypeScript SDKs do this internally.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `dirsql::vtab::load_module` | `load_module(&conn)?` | `let scope = StatementScope::new(); load_module(&conn, Arc::clone(&scope))?` |
| `dirsql::parsed_vtab::load_module` | `load_module(&conn)?` | `load_module(&conn, Arc::clone(&scope))?` (the same scope for every module on one connection) |
| Running statements against the registered tables | nothing between statements | `scope.reset()` before each statement, or `let _guard = scope.enter();` for its duration |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Within one statement, every reference to a path table sees the rows of the
  statement's first scan of it. A file that appears or vanishes while a
  statement runs is seen by the next statement, not by the current one's
  later cursors.
- Rows come out in the same path order as before; the `walkdir` crate is no
  longer a dependency.

#### Verification

```bash
cd "$(mktemp -d)"
printf 'a\n' > a.md
printf 'b\n' > b.md
dirsql query "WITH x AS (SELECT count(*) AS n FROM './') SELECT n FROM x UNION ALL SELECT count(*) FROM './'"
# expected: [{"n":2},{"n":2}] -- one walk, both arms agree
```
