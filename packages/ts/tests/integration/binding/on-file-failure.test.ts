// Binding-tier tests: any per-file `onFile` failure fails `ready` and the
// query, with an error naming the file, the table and the original error.
import { mkdirSync, writeFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

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

    for (const call of [() => db.ready, () => db.query("SELECT * FROM notes")]) {
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
    const db = mkdb(() => [{ n: null }], "CREATE TABLE notes (n INTEGER NOT NULL)");

    const err = await db.ready.then(
      () => null,
      (e: Error) => e,
    );
    expect(err?.message).toContain("d/a.txt");
    expect(err?.message).toContain("notes");
    expect(err?.message).toContain("NOT NULL");
  });
});
