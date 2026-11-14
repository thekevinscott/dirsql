// E2E: every write reaches a long-lived `DirSQL`'s index.
// Regressions for the reported repros: a declared table read with no watch
// running, a change written between `ready` and `watch()` starting, and a
// query racing a consumed `watch()`. No mocks: real addon, real filesystem,
// real SQLite.

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
import { isDeepStrictEqual } from "node:util";
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

// A file caught mid-write fails its hook, and every query with it, until the
// write completes.
async function awaitEq<T>(read: () => Promise<T>, expected: T): Promise<void> {
  let seen: T | Error = new Error("never read");
  for (let i = 0; i < 1000; i++) {
    seen = await read().catch((e: Error) => e);
    if (isDeepStrictEqual(seen, expected)) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  expect(seen).toEqual(expected);
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

describe.runIf(process.platform === "linux")(
  "every write reaches a long-lived DirSQL's index",
  () => {
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
        const open = db;
        await awaitEq(() => turnIds(open), ["turns/0", "turns/1"]);
      });

      it("reads a create, an edit and a delete", async () => {
        writeFileSync(join(dir, "a.json"), JSON.stringify({ v: 1 }));
        db = recordsDb(dir);
        await db.ready;
        expect(await records(db)).toEqual([["a.json", 1]]);
        writeFileSync(join(dir, "b.json"), JSON.stringify({ v: 2 }));
        writeFileSync(join(dir, "a.json"), JSON.stringify({ v: 10 }));
        const open = db;
        await awaitEq(
          () => records(open),
          [
            ["a.json", 10],
            ["b.json", 2],
          ],
        );
        unlinkSync(join(dir, "b.json"));
        await awaitEq(() => records(open), [["a.json", 10]]);
      });
    });

    describe("a change written before watch starts", () => {
      it(
        "reaches the index and no stream",
        async () => {
          addTurn(dir, 0);
          db = turnsDb(dir);
          await db.ready;
          addTurn(dir, 1);
          const open = db;
          await awaitEq(() => turnIds(open), ["turns/0", "turns/1"]);
          const stream = db.watch()[Symbol.asyncIterator]();
          addTurn(dir, 2);
          const next = await stream.next();
          expect(next.done).toBe(false);
          expect([next.value.action, next.value.row?.id]).toEqual([
            "insert",
            "turns/2",
          ]);
          expect(await turnIds(db)).toEqual(["turns/0", "turns/1", "turns/2"]);
        },
        TEST_TIMEOUT,
      );
    });

    describe("a query racing a consumed watch", () => {
      it(
        "gets every write into the index",
        async () => {
          mkdirSync(join(dir, "turns"));
          db = turnsDb(dir);
          await db.ready;
          consume(db);
          const open = db;
          for (let n = 0; n < RACE_ROUNDS; n++) {
            addTurn(dir, n);
            await awaitEq(
              async () => (await open.query("SELECT id FROM turns")).length,
              n + 1,
            );
          }
        },
        TEST_TIMEOUT,
      );

      it("gets a create into the index", async () => {
        writeFileSync(join(dir, "a.json"), JSON.stringify({ v: 1 }));
        db = recordsDb(dir);
        await db.ready;
        consume(db);
        writeFileSync(join(dir, "b.json"), JSON.stringify({ v: 2 }));
        const open = db;
        await awaitEq(
          () => records(open),
          [
            ["a.json", 1],
            ["b.json", 2],
          ],
        );
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
  },
);
