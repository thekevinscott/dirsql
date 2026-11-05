### Watch rows compare after column affinity

#### Summary

Watch events (`poll_events`, the server's `GET /events`) now compare and report rows after SQLite column affinity. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A re-run whose rows store the same values emits no events, where it used to emit an `update` per row whenever the hook's value type differed from the column's affinity. An event's row now holds the stored value: `"s":"5"` for a `TEXT` column the hook filled with `5`, `"x":1.0` for a `REAL` column filled with `1`.

#### Verification

With `CREATE TABLE t (k TEXT PRIMARY KEY, s TEXT)` filled by a hook emitting `{"k":"a","s":5}`, rewrite the file with the same content while `dirsql server` streams `/events`: no `row` event arrives.
