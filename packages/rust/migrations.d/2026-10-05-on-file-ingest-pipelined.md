### Core: on-file ingest overlaps parsing and storing

#### Summary

A `[[table]]` `on-file` hook's rows are stored chunk by chunk while the hook
runs on the next batch of files, and text values reach SQLite without a
copy. Rows, errors and query results are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

_None._
