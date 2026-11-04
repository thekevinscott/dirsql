### Path-tables differing only in letter case are distinct

#### Summary

A path-table name now selects exactly the files it spells, even when another path-table in the same statement or session differs from it only in letter case. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

`SELECT path FROM './up'` after `SELECT path FROM './Up'` returns the files under `up/` instead of those under `Up/`, and `FROM './Up' u, './up' l` joins the two directories. A column qualified by a case variant that its statement does not read (`SELECT "./up".path FROM './Up'`) now fails with `no such column` instead of reading the other spelling's table.

#### Verification

In a directory holding `a.md` and `b.MD`, `dirsql "SELECT path FROM './*.md' UNION ALL SELECT path FROM './*.MD'"` prints `[{"path":"a.md"},{"path":"b.MD"}]`.
