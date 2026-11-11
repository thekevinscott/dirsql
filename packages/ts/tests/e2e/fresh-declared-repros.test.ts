// E2E: a long-lived `DirSQL` reads what is on disk when each query runs.
// Regressions for the reported repros: a declared table read with no watch
// running, a change written between `ready` and `watch()` starting, and a
// query racing a consumed `watch()`. Every query runs the moment the write
// before it returns. No mocks: real addon, real filesystem, real SQLite.

import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative, sep } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const RACE_ROUNDS = 1000;
const TEST_TIMEOUT = 120_000;

function turnsDb(root: string): DirSQL {
  return new DirSQL({
    root,
    noIgnore: true,
    tables: [
      {
        name: "turns",
        ddl: "CREATE TABLE turns (id TEXT PRIMARY KEY)",
        glob: "turns/*/a.txt",
        onFile: (filePath: string) => [
          { id: relative(root, dirname(filePath)).split(sep).join("/") },
        ],
      },
    ],
  });
}

function recordsDb(root: string): DirSQL {
  return new DirSQL({
    root,
    tables: [
      {
        name: "t",
        ddl: "CREATE TABLE t (name TEXT, v INTEGER)",
        glob: "*.json",
        onFile: (filePath: string) => [
          {
            name: basename(filePath),
            v: JSON.parse(readFileSync(filePath, "utf8")).v,
          },
        ],
      },
    ],
  });
}

function addTurn(root: string, n: number): void {
  mkdirSync(join(root, "turns", String(n)), { recursive: true });
  writeFileSync(join(root, "turns", String(n), "a.txt"), "x");
}

async function turnIds(db: DirSQL): Promise<string[]> {
  const rows = await db.query("SELECT id FROM turns ORDER BY id");
  return rows.map((row) => String(row.id));
}

async function records(db: DirSQL): Promise<[string, number][]> {
  const rows = await db.query("SELECT name, v FROM t ORDER BY name");
  return rows.map((row) => [String(row.name), Number(row.v)]);
}

// Closing the instance ends the stream by rejecting its pending `next()`.
function consume(db: DirSQL): void {
  (async () => {
    for await (const _ of db.watch()) {
      // drained
    }
  })().catch((error: Error) => {
    if (!error.message.includes("instance closed")) {
      throw error;
    }
  });
}

// A query reads a write at once only where it drains the inotify queue.
describe.runIf(process.platform === "linux")("a long-lived DirSQL reads what is on disk", () => {
  let dir: string;
  let db: DirSQL | undefined;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-fresh-repro-"));
  });

  afterEach(() => {
    db?.close();
    db = undefined;
    rmSync(dir, { recursive: true, force: true });
  });

  describe("with no watch running", () => {
    it("reads a file written after ready", async () => {
      addTurn(dir, 0);
      db = turnsDb(dir);
      await db.ready;
      addTurn(dir, 1);
      expect(await turnIds(db)).toEqual(["turns/0", "turns/1"]);
    });

    it("reads a create, an edit and a delete", async () => {
      writeFileSync(join(dir, "a.json"), JSON.stringify({ v: 1 }));
      db = recordsDb(dir);
      await db.ready;
      expect(await records(db)).toEqual([["a.json", 1]]);
      writeFileSync(join(dir, "b.json"), JSON.stringify({ v: 2 }));
      writeFileSync(join(dir, "a.json"), JSON.stringify({ v: 10 }));
      expect(await records(db)).toEqual([
        ["a.json", 10],
        ["b.json", 2],
      ]);
      unlinkSync(join(dir, "b.json"));
      expect(await records(db)).toEqual([["a.json", 10]]);
    });
  });

  describe("a change written before watch starts", () => {
    it(
      "is read by the next query and streamed once",
      async () => {
        addTurn(dir, 0);
        db = turnsDb(dir);
        await db.ready;
        addTurn(dir, 1);
        expect(await turnIds(db)).toEqual(["turns/0", "turns/1"]);
        const stream = db.watch()[Symbol.asyncIterator]();
        // Unread by any query, so only the watcher reports it, after every
        // change before it.
        addTurn(dir, 2);
        const inserted: string[] = [];
        for (;;) {
          const next = await stream.next();
          expect(next.done).toBe(false);
          expect(next.value.action).toBe("insert");
          const id = String(next.value.row?.id);
          inserted.push(id);
          if (id === "turns/2") {
            break;
          }
        }
        expect(inserted).toEqual(["turns/1", "turns/2"]);
        expect(await turnIds(db)).toEqual(["turns/0", "turns/1", "turns/2"]);
      },
      TEST_TIMEOUT,
    );
  });

  describe("a query racing a consumed watch", () => {
    it(
      "reads every write before it",
      async () => {
        mkdirSync(join(dir, "turns"));
        db = turnsDb(dir);
        await db.ready;
        consume(db);
        const misses: number[] = [];
        for (let n = 0; n < RACE_ROUNDS; n++) {
          addTurn(dir, n);
          if ((await db.query("SELECT id FROM turns")).length !== n + 1) {
            misses.push(n);
          }
        }
        expect(misses).toEqual([]);
      },
      TEST_TIMEOUT,
    );

    it("reads a create", async () => {
      writeFileSync(join(dir, "a.json"), JSON.stringify({ v: 1 }));
      db = recordsDb(dir);
      await db.ready;
      consume(db);
      writeFileSync(join(dir, "b.json"), JSON.stringify({ v: 2 }));
      expect(await records(db)).toEqual([
        ["a.json", 1],
        ["b.json", 2],
      ]);
    });
  });

  describe("a path table", () => {
    it("reads a create, an edit and a delete", async () => {
      writeFileSync(join(dir, "a.json"), JSON.stringify({ v: 1 }));
      db = new DirSQL({ root: dir, noIgnore: true });
      await db.ready;
      const sql = "SELECT path, content FROM './*.json' ORDER BY path";
      const read = async () =>
        (await db?.query(sql))?.map((row) => [row.path, row.content]);
      expect(await read()).toEqual([["a.json", '{"v":1}']]);
      writeFileSync(join(dir, "b.json"), JSON.stringify({ v: 2 }));
      writeFileSync(join(dir, "a.json"), JSON.stringify({ v: 10 }));
      expect(await read()).toEqual([
        ["a.json", '{"v":10}'],
        ["b.json", '{"v":2}'],
      ]);
      unlinkSync(join(dir, "b.json"));
      expect(await read()).toEqual([["a.json", '{"v":10}']]);
    });
  });
});
