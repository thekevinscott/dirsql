### Rust SDK: README table examples use the current `Table` signature

**Summary**

The README's table examples and the sentence introducing them now show
`Table::new(name, ddl, glob, on_file)`. They showed `Table::new(ddl, glob, extract)`, which does not compile.
README only: no code, config, or CLI surface changed, and nothing breaks.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

_None._

**Verification**

```bash
grep -c '^ *"posts",$' README.md
# 2
```
