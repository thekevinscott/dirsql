### Faster wide-directory listings

#### Summary

A wide directory's rows are made on the threads that judge its entries, and on Linux its entries are read straight from the kernel's buffer. Query results and their order are unchanged; nothing breaks.

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
