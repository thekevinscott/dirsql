// Integration tests for the `watch()` async-iterator API (#294 test parity).
//
// Mirrors packages/python/tests/integration/async_dirsql_test.py
// (describe_watch) and packages/rust/tests/sdk.rs
// (it_streams_watch_*_events): the TS SDK exposes the same event stream as
// `for await (const event of db.watch())`, which was previously only covered
// via the lower-level `startWatcher()` + `pollEvents()` primitives
// (index.test.ts). Real napi binding, real Rust core, real temp filesystem —
// nothing mocked.

import { readFileSync } from "node:fs";
import { mkdir, mkdtemp, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL, type RowEvent } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const TEST_TIMEOUT = 15_000;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// Consume `db.watch()` until `until` is satisfied, keeping only events that
// pass `filter`. The iterator never self-terminates; if the expected events
// never arrive, the enclosing test times out (the vitest analog of the
// Python suite's `asyncio.wait_for(...)` + `pytest.fail`).
async function collectFromWatch(
  db: DirSQL,
  opts: {
    filter?: (event: RowEvent) => boolean;
    until: (events: RowEvent[]) => boolean;
  },
): Promise<RowEvent[]> {
  const events: RowEvent[] = [];
  for await (const event of db.watch()) {
    if (opts.filter && !opts.filter(event)) {
      continue;
    }
    events.push(event);
    if (opts.until(events)) {
      break;
    }
  }
  return events;
}

function jsonTable(glob: string) {
  return {
    ddl: "CREATE TABLE items (name TEXT)",
    glob,
    extract: (filePath: string) => [JSON.parse(readFileSync(filePath, "utf8"))],
  };
}

describe("DirSQL watch() async iterator", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-watch-iter-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it(
    "emits insert events for new files",
    async () => {
      const db = new DirSQL({ root: dir, tables: [jsonTable("**/*.json")] });
      await db.ready;

      const collector = collectFromWatch(db, {
        filter: (event) => event.action === "insert",
        until: (events) => events.length >= 1,
      });

      // Give the watcher time to start.
      await sleep(300);

      // Create the file atomically — write to a sibling tmp path then rename
      // into place. Without this the watcher can fire on the partially
      // written file, producing a spurious error event ahead of the insert.
      const final = join(dir, "new_item.json");
      const tmp = `${final}.tmp`;
      await writeFile(tmp, JSON.stringify({ name: "apple" }));
      await rename(tmp, final);

      const events = await collector;
      expect(events.length).toBeGreaterThanOrEqual(1);
      expect(events[0].action).toBe("insert");
      expect(events[0].table).toBe("items");
      expect(events[0].row?.name).toBe("apple");
      expect(events[0].filePath).toBeTruthy();
    },
    TEST_TIMEOUT,
  );

  it(
    "emits delete events for removed files",
    async () => {
      await writeFile(
        join(dir, "doomed.json"),
        JSON.stringify({ name: "doomed" }),
      );

      const db = new DirSQL({ root: dir, tables: [jsonTable("**/*.json")] });
      expect(await db.query("SELECT * FROM items")).toHaveLength(1);

      const collector = collectFromWatch(db, {
        filter: (event) => event.action === "delete",
        until: (events) => events.length >= 1,
      });
      await sleep(300);

      await rm(join(dir, "doomed.json"));

      const events = await collector;
      expect(events[0].action).toBe("delete");
      expect(events[0].table).toBe("items");
      expect(events[0].row?.name).toBe("doomed");

      // The database reflects the deletion.
      expect(await db.query("SELECT * FROM items")).toHaveLength(0);
    },
    TEST_TIMEOUT,
  );

  it(
    "emits update events for modified files",
    async () => {
      await writeFile(
        join(dir, "item.json"),
        JSON.stringify({ name: "draft" }),
      );

      const db = new DirSQL({ root: dir, tables: [jsonTable("**/*.json")] });
      await db.ready;

      const collector = collectFromWatch(db, {
        // Mid-write the watcher can deliver a spurious error event before
        // the real diff lands; only update / delete / insert matter here.
        filter: (event) =>
          event.action === "update" ||
          event.action === "delete" ||
          event.action === "insert",
        until: (events) => events.length >= 1,
      });
      await sleep(300);

      await writeFile(
        join(dir, "item.json"),
        JSON.stringify({ name: "final" }),
      );

      const events = await collector;
      // Could surface as a single update or a delete+insert pair depending
      // on the diff logic.
      const actions = new Set(events.map((e) => e.action));
      expect(
        actions.has("update") ||
          (actions.has("delete") && actions.has("insert")),
      ).toBe(true);
    },
    TEST_TIMEOUT,
  );

  it(
    "emits error events with table attribution when extract fails",
    async () => {
      const db = new DirSQL({ root: dir, tables: [jsonTable("**/*.json")] });
      await db.ready;

      const collector = collectFromWatch(db, {
        filter: (event) => event.action === "error",
        until: (events) => events.length >= 1,
      });
      await sleep(300);

      await writeFile(join(dir, "bad.json"), "not json at all");

      const events = await collector;
      expect(events[0].action).toBe("error");
      expect(events[0].error).toBeTruthy();
      // The failing file matched the `items` table's glob; the error event
      // must carry that attribution so multi-table consumers can route the
      // error to the right handler.
      expect(events[0].table).toBe("items");
    },
    TEST_TIMEOUT,
  );

  it(
    "keeps the database in sync with file changes",
    async () => {
      const db = new DirSQL({ root: dir, tables: [jsonTable("**/*.json")] });
      expect(await db.query("SELECT * FROM items")).toHaveLength(0);

      const collector = collectFromWatch(db, {
        filter: (event) => event.action === "insert",
        until: (events) => events.length >= 1,
      });
      await sleep(300);

      const final = join(dir, "new.json");
      const tmp = `${final}.tmp`;
      await writeFile(tmp, JSON.stringify({ name: "added" }));
      await rename(tmp, final);

      await collector;

      const rows = await db.query("SELECT * FROM items");
      expect(rows).toHaveLength(1);
      expect(rows[0].name).toBe("added");
    },
    TEST_TIMEOUT,
  );

  // Docs (guide/watching.md "How diffing works"): a file shrinking from 3
  // rows to 2 must end with the dropped row deleted. Mirrors the Python
  // docs-gaps test (it_emits_delete_for_shrinking_file_positionally); the
  // same doc/impl divergence note applies — the current core does a full
  // replace on shrink (packages/rust/src/differ.rs::diff_rows), so we assert
  // only that a delete for the dropped row appears and the end state is
  // correct, without contradicting either mechanism.
  it(
    "emits a delete for the dropped row when a file shrinks",
    async () => {
      const path = join(dir, "rows.jsonl");
      const line = (i: number) =>
        `${JSON.stringify({ idx: i, name: `row-${i}` })}\n`;
      await writeFile(path, line(0) + line(1) + line(2));

      const db = new DirSQL({
        root: dir,
        tables: [
          {
            ddl: "CREATE TABLE rows (idx INTEGER, name TEXT)",
            glob: "*.jsonl",
            extract: (filePath: string) =>
              readFileSync(filePath, "utf8")
                .split("\n")
                .filter((l) => l.length > 0)
                .map((l) => JSON.parse(l)),
          },
        ],
      });
      expect(await db.query("SELECT * FROM rows")).toHaveLength(3);

      const collector = collectFromWatch(db, {
        until: (events) =>
          events.some((e) => e.action === "delete" && e.row?.name === "row-2"),
      });
      await sleep(300);

      // Shrink from 3 -> 2 rows (drop the third).
      await writeFile(path, line(0) + line(1));

      const events = await collector;
      const deletedNames = new Set(
        events.filter((e) => e.action === "delete").map((e) => e.row?.name),
      );
      expect(deletedNames.has("row-2")).toBe(true);

      // The database reflects only the two surviving rows.
      const post = await db.query("SELECT * FROM rows ORDER BY idx");
      expect(post.map((r) => r.idx)).toEqual([0, 1]);
    },
    TEST_TIMEOUT,
  );
});
