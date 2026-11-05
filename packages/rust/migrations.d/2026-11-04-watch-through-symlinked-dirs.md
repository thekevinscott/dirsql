### Watch tracks files reached through a symlinked directory

#### Summary

While watching, a file indexed through a symlinked directory inside the root changes with its target, as a fresh scan of the tree would show. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

The event stream now carries an insert, update, or delete for `link/x.json` alongside the one for `real/x.json` when `link -> real` and `real/x.json` changes. A per-table `on-file` hook no longer reports an error for a dangling `link/x.json` after `real` leaves the tree.

#### Verification

With `real/x.json`, `link -> real`, and a table globbing `*/*.json`, run `dirsql server`, subscribe to `/events`, and `mv real /tmp/elsewhere`: two `delete` events arrive, for `link/x.json` and `real/x.json`.
