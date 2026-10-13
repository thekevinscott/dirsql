### Faster recursive walks

#### Summary

The tree walk builds each entry's path in one allocation, so its threads contend the allocator less. Query results and their order are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT count(*) FROM './*/*.md'"
```

Prints the same count as before, sooner.
