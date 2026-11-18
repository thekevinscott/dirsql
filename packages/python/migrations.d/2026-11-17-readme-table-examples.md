### Python SDK: README table examples use the current `Table` signature

**Summary**

The README's table examples and the sentence introducing them now show
`Table(name=..., ddl=..., glob=..., on_file=...)`. They showed `Table(ddl=..., glob=..., extract=...)`, which raises `TypeError`.
README only: no code, config, or CLI surface changed, and nothing breaks.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

_None._

**Verification**

```bash
python -c "from dirsql import Table; print(Table(name='t', ddl='CREATE TABLE t (a TEXT)', glob='*.json', on_file=lambda p: []).glob)"
# *.json
```
