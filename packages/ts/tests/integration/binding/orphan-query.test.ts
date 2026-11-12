// Binding-tier tests: while a child references no parent, every query raises
// the foreign-key error, until the parent appears or the child goes. Linux
// only: there a query applies every event the kernel has queued first.
import { mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const stem = (filePath: string) => basename(filePath, ".txt");

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

    expect(
      await live.query(
        "SELECT c.id AS child, p.id AS parent FROM child c JOIN parent p ON p.id = c.parent",
      ),
    ).toEqual([{ child: "lost", parent: "p1" }]);
  });

  it("clears when the child is deleted", async () => {
    const live = db as DirSQL;
    await queryError(live);

    unlinkSync(join(dir, "children", "lost.txt"));

    expect(await live.query("SELECT id FROM child")).toEqual([]);
  });
});
