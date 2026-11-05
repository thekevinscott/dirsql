### Queries wait for an open watch batch

#### Summary

A query whose instance has fresh reads off now waits for any watch batch in progress to commit or roll back before it reads, as queries with fresh reads on already did. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A query issued during a watch batch returns the index as it stood before the batch or after it, never in between; it may wait for that batch's hooks to finish.

#### Verification

With two tables `a` and `b` over `d/*.jsonl` whose `on-file` is `sh -c 'sleep 1; cat "$@"' sh`, run `dirsql server`, write a new `d/2.jsonl`, and query `SELECT (SELECT count(*) FROM a) AS na, (SELECT count(*) FROM b) AS nb` repeatedly: every answer has `na` equal to `nb`.
