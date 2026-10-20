### Wide directories sort while they are read

#### Summary

A directory of more than 8192 entries is sorted in batches while it is read, then merged on all cores. Query results and their order are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT path FROM './*'"
```

Prints the same rows as before, sooner.
