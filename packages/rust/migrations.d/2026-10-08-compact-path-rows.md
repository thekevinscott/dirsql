### Compact path-table rows

#### Summary

A path-table row now takes about 100 bytes instead of about 180, so large scans use less memory and run a few percent faster. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

Query output is unchanged.

#### Verification

Run `dirsql query "SELECT path FROM './*'"` in a large directory; expect the same rows as before.
