### Lone surrogate escapes in on-file output name the escape

#### Summary

An on-file hook line holding a lone surrogate escape fails the table as before, with a clearer error naming the line and the escape. No API or configuration changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

The error for such a line reads `line <n>: lone surrogate \uXXXX is not valid Unicode` where it read `line <n> is not a JSON object: unexpected end of hex escape ...` (or `lone leading surrogate in hex escape ...`).

#### Verification

In an empty directory, write `{"a":"\ud800"}` to `s.ndjson` and run `dirsql query "SELECT a FROM './s.ndjson'" --on-file cat`; it exits `1` and stderr contains `line 1: lone surrogate \ud800 is not valid Unicode`.
