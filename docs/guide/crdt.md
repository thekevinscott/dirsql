# Collaboration with CRDTs

`dirsql` treats the filesystem as the source of truth. That works well when a single process (or a single human) is writing, but breaks down for multi-writer collaboration: two peers editing the same file concurrently produce a merge conflict, not a merged result.

[Conflict-free Replicated Data Types](https://crdt.tech/) (CRDTs) solve that merge problem at the data-structure level, not the filesystem level. Two replicas that apply the same set of edits -- in any order, with any network partitions in between -- converge on the same final state, without a central arbiter.

This guide is **opinionated**. It picks one library, explains the integration pattern with `dirsql`, and names the alternatives so you can steer if your situation is different.

## Recommendation: Automerge

Use [Automerge](https://automerge.org/) (specifically the 2.x series with [automerge-repo](https://automerge.org/automerge-repo/) for sync).

Why Automerge over the alternatives:

- **JSON-shaped document model.** Automerge docs look like nested maps, lists, and text. That matches `dirsql`'s one-object-per-file workflow -- each Automerge document is the thing your `extract` function projects into rows.
- **Cross-language SDKs that mirror `dirsql`'s parity story.** First-class Rust ([`automerge`](https://crates.io/crates/automerge)), TypeScript ([`@automerge/automerge`](https://www.npmjs.com/package/@automerge/automerge)), and Python ([`automerge`](https://pypi.org/project/automerge/)) implementations exist today, all driven by the same Rust core. If you already have all three `dirsql` SDKs in play, Automerge won't force a language monoculture on you.
- **Filesystem-friendly sync primitives.** `automerge-repo` ships a [`NodeFSStorageAdapter`](https://automerge.org/docs/repositories/storage/) that shards document history into regular files under a directory. That directory is exactly the kind of tree `dirsql` is designed to watch.
- **Binary format with a deterministic JSON view.** You never hand-edit a CRDT file, but every replica projects the same canonical JSON from the binary state. That canonical JSON is what you feed to `dirsql`'s extract function, so two peers that have synced will produce identical rows.

## The integration shape

There are two files per logical "document":

```
workspace/
  posts/
    hello/
      doc.automerge   <-- binary CRDT state (the source of truth for writers)
      view.json       <-- materialized JSON snapshot (written on each merge)
```

- Writers (editors, sync peers, etc.) mutate `doc.automerge` through Automerge APIs.
- After every mutation, the writer serializes the current document to `view.json`. This is the file `dirsql` indexes.
- `dirsql` watches `view.json`, not `doc.automerge`. The CRDT file is an implementation detail of how the JSON got there.

This keeps `dirsql`'s extract function oblivious to CRDT semantics: it reads a plain JSON file, exactly as it would without Automerge.

::: tip Why not `extract` directly from `.automerge`?
You could -- the Rust and Python Automerge SDKs let you load a binary doc and walk its fields. But it couples your table schema to the CRDT library version, makes `extract` non-pure (it allocates CRDT state on every file change), and buys nothing: the writer is the only place that can produce a valid Automerge blob, so it might as well produce the JSON view at the same time.
:::

### Example: posts as Automerge documents

::: code-group

```python [Python]
from dirsql import DirSQL, Table
import json

db = DirSQL(
    "./workspace",
    tables=[
        Table(
            ddl="CREATE TABLE posts (id TEXT, title TEXT, body TEXT, updated INTEGER)",
            # Match the JSON view, not the raw CRDT binary.
            glob="posts/*/view.json",
            extract=lambda path, content: [json.loads(content)],
        ),
    ],
)
```

```rust [Rust]
use dirsql::{DirSQL, Table};
// See `row_from_json` in getting-started.md.

let db = DirSQL::new(
    "./workspace",
    vec![
        Table::new(
            "CREATE TABLE posts (id TEXT, title TEXT, body TEXT, updated INTEGER)",
            "posts/*/view.json",
            |_path, content| vec![row_from_json(content)],
        ),
    ],
)?;
```

```typescript [TypeScript]
import { DirSQL, type TableDef } from 'dirsql';

const tables: TableDef[] = [
  {
    ddl: 'CREATE TABLE posts (id TEXT, title TEXT, body TEXT, updated INTEGER)',
    glob: 'posts/*/view.json',
    extract: (_path, content) => [JSON.parse(content)],
  },
];

const db = new DirSQL('./workspace', tables);
```

:::

The Automerge writer (sketch, TypeScript):

```typescript
import * as Automerge from '@automerge/automerge';
import { writeFileSync, readFileSync } from 'node:fs';

// Load (or create) the CRDT doc.
const bytes = readFileSync('workspace/posts/hello/doc.automerge');
let doc = Automerge.load<Post>(bytes);

// Apply an edit.
doc = Automerge.change(doc, (d) => {
  d.title = 'Hello, world';
  d.updated = Date.now();
});

// Persist both the CRDT state and the materialized view.
writeFileSync('workspace/posts/hello/doc.automerge', Automerge.save(doc));
writeFileSync('workspace/posts/hello/view.json', JSON.stringify(doc));
```

`dirsql`'s watcher picks up the change to `view.json`, re-runs `extract`, and emits an `update` row event. Queries over `posts` reflect the merged state without `dirsql` knowing Automerge exists.

## Multi-writer, in practice

1. Each peer runs an `automerge-repo` instance with the filesystem storage adapter pointed at its local `workspace/`.
2. Peers sync via any transport `automerge-repo` supports ([WebSocket](https://automerge.org/docs/repositories/networking/), [BroadcastChannel](https://automerge.org/docs/repositories/networking/), or a custom adapter).
3. On every sync, the repo applies incoming ops to the local CRDT, writes the updated `doc.automerge`, and the writer code re-serializes `view.json`.
4. Every peer's `dirsql` sees the same eventual `view.json` and produces the same rows.

The key invariant: **`view.json` is a deterministic projection of `doc.automerge`**. Two peers that have converged on the CRDT state must produce byte-identical (or at least semantically-identical) JSON views. Otherwise you get spurious `update` events that flap with sync order. Use `JSON.stringify` with sorted keys (or `json.dumps(..., sort_keys=True)` in Python) to guarantee this.

## Tradeoffs vs plain files

When **plain files** are the right answer:

- Single writer. A solo user editing `posts/*.json` will never hit a merge conflict. Adding a CRDT is overhead.
- Human-readable history matters. `git diff` on a JSON file tells a story; `git diff` on a CRDT binary does not.
- Schema churn is frequent. Renaming a field in plain JSON is a `sed`; in a CRDT it's a migration.

When **CRDTs** earn their complexity:

- Multi-writer without a central server (local-first, peer-to-peer).
- Offline edits that need to merge on reconnect.
- Fine-grained collaborative editing (cursor-level merging of a shared text field).

Hybrid is common: keep configuration and reference data as plain files, and use CRDTs only for the documents that genuinely have multiple writers.

## Alternatives we considered

- [**Yjs**](https://docs.yjs.dev/) -- the dominant JS CRDT, excellent for rich-text collaboration (it backs many of the production collab editors you've used). Skipped as the primary recommendation because its Rust port ([`yrs`](https://crates.io/crates/yrs)) and Python bindings lag the JS implementation. If your workload is browser-first and text-heavy, prefer Yjs.
- [**Loro**](https://loro.dev/) -- Rust-first CRDT with a clean API and good cross-language story. Worth watching; we'd consider it once its Python bindings are GA. Try it if you're Rust-centric and don't need Automerge's ecosystem.
- **Operational Transform / hand-rolled merge logic** -- don't. OT is correct but hard to implement right, and you lose the offline-peer story that CRDTs give you for free.
- **Git as the merge engine** -- tempting because `dirsql` already lives on the filesystem, but three-way merges of structured JSON produce garbage conflict markers that no extract function can parse. Use a CRDT.

## See also

- [Ink & Switch's local-first essay](https://www.inkandswitch.com/local-first/) -- the design space CRDTs sit in.
- [Automerge documentation](https://automerge.org/docs/) -- API reference and sync-adapter guides.
- [`crdt.tech`](https://crdt.tech/) -- library survey across languages.
