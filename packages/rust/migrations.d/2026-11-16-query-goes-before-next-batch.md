### A waiting query is answered before the next batch

#### Summary

A query that arrives while a batch of file changes is being applied is answered as soon as that batch ends, before the next batch is applied. No API or configuration changes are required and nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

Queries and batches take the index in the order they asked for it. A query that arrives during a batch waits for that batch only; it is no longer passed over for the batches that follow. A batch is never split, so under a writer that never pauses a query can still wait as long as the batch in progress takes.

#### Verification

_None._
