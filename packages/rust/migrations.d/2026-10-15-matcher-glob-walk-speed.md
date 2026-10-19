### Faster recursive globs with a narrow file name

#### Summary

The tree walk rejects a file by its name before building its path, checks for `.git` and `.gitignore` in the directory listing it already read, and reads directories ahead of the output in the order the output reaches them. Query results and their order are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT count(*) FROM './**/file?.md'"
```

Prints the same count as before, sooner.
