### Every TypeScript watch stream receives every event

#### Summary

Multiple `DirSQL.watch()` streams now each receive every event, matching Python. Single-stream usage is unchanged; no API changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

Streams on one instance now receive every event observed after their creation, including events buffered before their first iteration. Stop consuming a stream with `break` or `return()` when finished.

#### Verification

Run this with Node in a project with `dirsql` installed:

```sh
node --input-type=module <<'JS'
import { DirSQL } from 'dirsql';
import { mkdtemp, writeFile, rename } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const root = await mkdtemp(join(tmpdir(), 'watch-broadcast-'));
const db = new DirSQL({ root, tables: [{
  name: 'items', ddl: 'CREATE TABLE items (n INTEGER)',
  glob: '*.json', onFile: () => [{ n: 1 }],
}] });
await db.ready;
const streams = [db.watch(), db.watch()];
const events = Promise.all(streams.map(stream => stream.next()));
await new Promise(resolve => setTimeout(resolve, 300));
await writeFile(join(root, 'a.tmp'), '{}');
await rename(join(root, 'a.tmp'), join(root, 'a.json'));
console.log((await events).map(event => event.value.action));
await Promise.all(streams.map(stream => stream.return()));
db.close();
JS
```

Expected output: `[ 'insert', 'insert' ]`.
