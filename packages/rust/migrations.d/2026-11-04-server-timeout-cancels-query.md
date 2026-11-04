### A server query that times out is cancelled

#### Summary

`dirsql server` now interrupts a query once it answers `408 Request Timeout`, so the query releases the index and later queries run at once. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

With `dirsql server --port 7392` running, `POST /query` with `{"sql": "WITH RECURSIVE r(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM r LIMIT 100000) SELECT count(*) AS n FROM r a, r b"}` returns `408` after 30 seconds, and an immediate `POST /query` with `{"sql": "SELECT path FROM './*'"}` returns `200` at once.
