### Python SDK: `watch=False` opts out of the watcher

**Summary**

`DirSQL(..., watch=False)` builds from one walk and starts no watcher; `watch()` raises `RuntimeError`. Additive: the option defaults to watching, so nothing breaks.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

_None._

**Verification**

```bash
python -c "import asyncio; from dirsql import DirSQL
async def m():
    db = DirSQL('.', watch=False); await db.ready()
    try: db.watch()
    except RuntimeError as e: print(e)
asyncio.run(m())"
# watching is off: this instance was built with watch disabled
```
