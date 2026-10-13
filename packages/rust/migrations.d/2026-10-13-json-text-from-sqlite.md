### Core: JSON text cells written from SQLite's buffer

#### Summary

The CLI's JSON output writes text cells without copying them first, and
text that is not valid UTF-8 prints with replacement characters instead of
crashing the query. Output for valid text is byte-identical; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A JSON-format query whose text cell is invalid UTF-8 used to panic; it now
prints the cell with U+FFFD in place of each invalid sequence.

#### Verification

```bash
cd "$(mktemp -d)"
dirsql query "SELECT CAST(x'ff41' AS TEXT) AS t" --format json
# expected: [{"t":"�A"}]
```
