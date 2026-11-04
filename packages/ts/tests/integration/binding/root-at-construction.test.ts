import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// The index root is read when `new DirSQL` is called, so a chdir before the
// background build runs does not move it.
describe("root at construction", () => {
  let dir: string;
  let original: string;

  beforeEach(async () => {
    original = process.cwd();
    dir = await mkdtemp(join(tmpdir(), "dirsql-root-at-construction-"));
  });

  afterEach(async () => {
    process.chdir(original);
    await rm(dir, { recursive: true, force: true });
  });

  it("roots a default instance at the cwd of construction", async () => {
    for (const who of ["A", "B"]) {
      await mkdir(join(dir, who));
      await writeFile(join(dir, who, `${who}.json`), "{}\n");
    }
    process.chdir(join(dir, "B"));
    const db = new DirSQL({});
    process.chdir(join(dir, "A"));

    expect(await db.query("SELECT path FROM './*.json'")).toEqual([
      { path: "B.json" },
    ]);
  });

  it("resolves a relative root against the cwd of construction", async () => {
    await mkdir(join(dir, "tree", "sub"), { recursive: true });
    await writeFile(join(dir, "tree", "a.json"), "{}\n");
    process.chdir(dir);
    const db = new DirSQL({ root: "tree" });
    process.chdir(join(dir, "tree", "sub"));

    expect(await db.query("SELECT path FROM './**'")).toEqual([
      { path: "a.json" },
    ]);
  });
});
