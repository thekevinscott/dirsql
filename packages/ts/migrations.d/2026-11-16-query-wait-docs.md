### Document how long a query waits while changes are being applied

#### Summary

Documentation only. `query`'s doc comment now says that a query arriving while a batch of file changes is being applied waits for that batch, and that under a writer that never pauses it waits as long as one batch takes. Nothing breaks and no code changes.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

Hover `db.query` in an editor, or open `dist/dirsql.d.ts`, and find "waits for that batch".
