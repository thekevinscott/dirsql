### Every Python `watch()` stream receives every event (#1397)

#### Summary

Several `DirSQL.watch()` streams on one Python instance each receive every event, instead of splitting them. A single stream behaves as before. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- With more than one `watch()` stream on one instance, each stream now yields every event observed after it was created. Code that hand-rolled fan-out over one stream keeps working.

#### Verification

```python
import asyncio, pathlib, tempfile
from dirsql import DirSQL, Table

async def main():
    root = pathlib.Path(tempfile.mkdtemp())
    table = Table(
        name="files",
        ddl="CREATE TABLE files (n INTEGER)",
        glob="*.json",
        on_file=lambda path: [{"n": 1}],
    )
    db = DirSQL(str(root), tables=[table])
    await db.ready()

    async def first(stream):
        async for event in stream:
            return event.action

    tasks = [asyncio.create_task(first(db.watch())) for _ in range(2)]
    await asyncio.sleep(0.3)
    (root / "a.json").write_text("{}")
    print(await asyncio.wait_for(asyncio.gather(*tasks), 5))  # ['insert', 'insert']

asyncio.run(main())
```
