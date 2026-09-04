## Summary

The `[[dirsql.function]]` worker response contract gains an optional top-level
`"meta"` object next to `"ok"`. dirsql reads one key from it today,
`{"meta": {"cached": true}}`, and reports those round trips as the `(N cached)`
split on the worker-call progress line.

## Required changes

_None._ The change is additive on the wire and in the parser: the response
parser has always read `err`, else `ok`, and ignored every other key, so a
worker written before this release keeps working byte-for-byte unchanged. The
break is in the *documented* contract — `docs/reference/config.md` enumerated
the response shape exhaustively — not in what any existing worker sends.

## Deprecations removed

_None._

## Behavior changes without code changes

- A worker that already emitted a top-level `"meta"` key for its own purposes
  now has `meta.cached` interpreted by dirsql. The only effect is on the
  progress line's `(N cached)` figure; the value bound into the query is still
  the `"ok"` field alone, and no query result changes.
- The worker-call summary line grows a parenthetical when a worker reports
  cache hits: `dirsql: ran 41231 worker calls in 2m41s (38104 cached)`. A
  worker that reports none gets the unchanged line — there is no `(0 cached)`.
  The line is stderr-only and still gated on `DIRSQL_PROGRESS` (terminal-only
  and slow-work-only by default), so piped and redirected runs are unaffected.
- Both figures count **worker round trips, not rows**: a `deterministic = true`
  function lets SQLite reuse an answer for identical arguments within a query,
  and those repeats never reach the worker.

## Verification

Declare a worker that flags its responses and run a query that calls it:

```console
$ DIRSQL_PROGRESS=always dirsql query "SELECT embed(basename) FROM './*.txt'"
dirsql: ran 4 worker calls in 0.0s (4 cached)
```

A worker that sends no `meta` prints `dirsql: ran 4 worker calls in 0.0s`.
