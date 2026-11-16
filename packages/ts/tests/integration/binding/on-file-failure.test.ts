// Binding-tier tests: any per-file `onFile` failure fails `ready` and the
// query, with an error naming the file, the table and the original error.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// The watcher's events reach the index after the write returns.
async function awaitQuery(
  db: DirSQL,
  done: (result: unknown) => boolean,
): Promise<void> {
  for (let i = 0; i < 1000; i++) {
    const result = await db.query("SELECT n FROM notes").catch((e: Error) => e);
    if (done(result)) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error("the query never answered as expected");
}

describe("DirSQL onFile failure", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-on-file-failure-"));
    mkdirSync(join(dir, "d"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  const mkdb = (onFile: (filePath: string) => unknown[], ddl: string) =>
    new DirSQL({
      root: dir,
      tables: [{ name: "notes", ddl, glob: "d/*.txt", onFile }],
    });

  it("fails ready and the query when the hook throws", async () => {
    writeFileSync(join(dir, "d", "a.txt"), "1");
    writeFileSync(join(dir, "d", "b.txt"), "oops");
    const db = mkdb((filePath) => {
      if (filePath.endsWith("b.txt")) {
        throw new Error("boom-xyzzy");
      }
      return [{ n: 1 }];
    }, "CREATE TABLE notes (n INTEGER)");

    for (const call of [
      () => db.ready,
      () => db.query("SELECT * FROM notes"),
    ]) {
      const err = await call().then(
        () => null,
        (e: Error) => e,
      );
      expect(err?.message).toContain("d/b.txt");
      expect(err?.message).toContain("notes");
      expect(err?.message).toContain("boom-xyzzy");
    }
  });

  it("fails ready on a NOT NULL violation", async () => {
    writeFileSync(join(dir, "d", "a.txt"), "1");
    const db = mkdb(
      () => [{ n: null }],
      "CREATE TABLE notes (n INTEGER NOT NULL)",
    );

    const err = await db.ready.then(
      () => null,
      (e: Error) => e,
    );
    expect(err?.message).toContain("d/a.txt");
    expect(err?.message).toContain("notes");
    expect(err?.message).toContain("NOT NULL");
  });

  it.runIf(process.platform === "linux")(
    "fails every query while a changed file fails, until it is fixed",
    async () => {
      writeFileSync(join(dir, "d", "a.txt"), "1");
      const db = mkdb((filePath) => {
        const text = readFileSync(filePath, "utf8");
        if (!/^\d+$/.test(text)) {
          throw new Error(`not a number: ${text}`);
        }
        return [{ n: Number(text) }];
      }, "CREATE TABLE notes (n INTEGER)");
      expect(await db.query("SELECT n FROM notes")).toEqual([{ n: 1 }]);

      writeFileSync(join(dir, "d", "a.txt"), "half-writ");
      await awaitQuery(
        db,
        (r) => r instanceof Error && r.message.includes("half-writ"),
      );
      for (let i = 0; i < 3; i++) {
        const err = await db.query("SELECT n FROM notes").then(
          () => null,
          (e: Error) => e,
        );
        expect(err?.message).toContain("d/a.txt");
        expect(err?.message).toContain("notes");
        expect(err?.message).toContain("half-writ");
      }

      writeFileSync(join(dir, "d", "a.txt"), "22");
      await awaitQuery(db, (r) => isDeepStrictEqual(r, [{ n: 22 }]));
      expect(await db.query("SELECT n FROM notes")).toEqual([{ n: 22 }]);
      expect(await db.query("SELECT n FROM notes")).toEqual([{ n: 22 }]);
    },
  );
});
