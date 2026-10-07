### POSIX classes match Unicode characters

#### Summary

POSIX classes in globs match Unicode characters, as bash does in a UTF-8 locale.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- POSIX classes such as `[[:alpha:]]`, `[[:upper:]]` and `[[:punct:]]` match Unicode characters instead of ASCII only: `./[[:alpha:]].md` now matches `é.md`. `digit`, `xdigit` and `ascii` stay ASCII.

#### Verification

```bash
dirsql query "SELECT path FROM './[[:alpha:]].md'"
```
