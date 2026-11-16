// Binding-tier tests: while a child references no parent, every query raises
// the foreign-key error, until the parent appears or the child goes.
import { mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const stem = (filePath: string) => basename(filePath, ".txt");

// Wait for the index to catch up: until `sql` raises, or succeeds.
async function settle(live: DirSQL, sql: string, raises: boolean) {
  for (let i = 0; i < 1000; i++) {
    const failed = await live.query(sql).then(
      () => false,
      () => true,
    );
    if (failed === raises) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

describe.runIf(process.platform === "linux")("a child with no parent", () => {
  let dir: string;
  let db: DirSQL | undefined;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-orphan-query-"));
    mkdirSync(join(dir, "parents"));
    mkdirSync(join(dir, "children"));
    db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "parent",
          ddl: "CREATE TABLE parent (id TEXT PRIMARY KEY)",
          glob: "parents/*.txt",
          onFile: (filePath) => [{ id: stem(filePath) }],
        },
        {
          name: "child",
          ddl: "CREATE TABLE child (id TEXT, parent TEXT REFERENCES parent(id))",
          glob: "children/*.txt",
          onFile: (filePath) => [
            { id: stem(filePath), parent: readFileSync(filePath, "utf8") },
          ],
        },
      ],
    });
    await db.ready;
    writeFileSync(join(dir, "children", "lost.txt"), "p1");
    await settle(db, "SELECT id FROM child", true);
  });

  afterEach(async () => {
    db?.close();
    db = undefined;
    await rm(dir, { recursive: true, force: true });
  });

  const queryError = async (live: DirSQL) => {
    const err = await live.query("SELECT id FROM child").then(
      (rows) => rows,
      (e: Error) => e,
    );
    expect(err).toBeInstanceOf(Error);
    expect((err as Error).message).toContain("foreign key violation");
    expect((err as Error).message).toContain("children/lost.txt");
  };

  it("fails every query until the parent is written", async () => {
    const live = db as DirSQL;
    await queryError(live);
    await queryError(live);

    writeFileSync(join(dir, "parents", "p1.txt"), "");
    const sql =
      "SELECT c.id AS child, p.id AS parent FROM child c JOIN parent p ON p.id = c.parent";
    await settle(live, sql, false);

    expect(await live.query(sql)).toEqual([{ child: "lost", parent: "p1" }]);
  });

  it("clears when the child is deleted", async () => {
    const live = db as DirSQL;
    await queryError(live);

    unlinkSync(join(dir, "children", "lost.txt"));
    await settle(live, "SELECT id FROM child", false);

    expect(await live.query("SELECT id FROM child")).toEqual([]);
  });
});
