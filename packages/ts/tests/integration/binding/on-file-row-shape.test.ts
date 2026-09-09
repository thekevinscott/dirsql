// Binding-tier tests for what `onFile` is allowed to return.
//
// A row must be an object. A primitive costs its own file and lands on the
// scan-failure list, the same way a throwing hook does since dirsql#714 --
// rather than inserting an all-NULL row or, for `null`, aborting the process.
import { writeFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("DirSQL onFile row shape", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-row-shape-"));
    writeFileSync(join(dir, "marker.json"), "{}");
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  const mkdb = (rows: unknown[]) =>
    new DirSQL({
      root: dir,
      tables: [
        {
          name: "t",
          ddl: "CREATE TABLE t (v)",
          glob: "*.json",
          onFile: () => rows as Record<string, unknown>[],
        },
      ],
    });

  const skipsTheFile = async (rows: unknown[]) => {
    const db = mkdb(rows);
    await db.ready;
    expect(await db.query("SELECT * FROM t")).toEqual([]);
    const failures = await db.scanFailures();
    expect(failures).toHaveLength(1);
    expect(basename(failures[0].path)).toBe("marker.json");
    return failures[0].message;
  };

  it("skips the file when a row is a number", async () => {
    expect(await skipsTheFile([1, 2])).toContain("array of objects");
  });

  it("skips the file when a row is a string", async () => {
    expect(await skipsTheFile(["a"])).toContain("array of objects");
  });

  it("skips the file when a row is null, rather than aborting the process", async () => {
    expect(await skipsTheFile([null])).toContain("array of objects");
  });

  it("skips the file when a row is a symbol, rather than aborting the process", async () => {
    expect(await skipsTheFile([Symbol("s")])).toContain("array of objects");
  });
});
