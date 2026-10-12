### Frontend state reports the query root and declared tables

#### Summary

The ready response of `GET /frontend/state` (served only with `dirsql server --frontend`) gains `root` and `tables` next to `error`. The Rust core gains `DirSQL::root()` and `DirSQL::tables()`. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A ready `GET /frontend/state` body is now `{"error": null, "root": "...", "tables": [...]}` rather than `{"error": null}`. `POST /query` is unchanged.

#### Verification

Run `dirsql server --frontend --port 0`, then `curl <printed URL>frontend/state`. The body lists `root` and `tables`.
