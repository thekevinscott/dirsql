// Binding-tier tests (real core, real fs) for fan-out file->table matching:
// a file matching N tables' globs populates all N tables; each table is an
// independent view over the files matching its glob (#580).

import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("DirSQL fan-out", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-fanout-"));
    await mkdir(join(dir, "data", "2401.00001"), { recursive: true });
    await writeFile(join(dir, "data", "2401.00001", "metadata.json"), "{}");
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("populates both tables with identical globs", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE ta (col_a TEXT)",
          glob: "data/*/metadata.json",
          onFile: () => [{ col_a: "A" }],
        },
        {
          ddl: "CREATE TABLE tb (col_b TEXT)",
          glob: "data/*/metadata.json",
          onFile: () => [{ col_b: "B" }],
        },
      ],
    });

    const a = await db.query("SELECT col_a FROM ta");
    expect(a).toEqual([{ col_a: "A" }]);
    const b = await db.query("SELECT col_b FROM tb");
    expect(b).toEqual([{ col_b: "B" }]);
  });

  it("populates both tables with overlapping distinct globs", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE ta (col_a TEXT)",
          glob: "data/*/metadata.json",
          onFile: () => [{ col_a: "A" }],
        },
        {
          ddl: "CREATE TABLE tb (col_b TEXT)",
          glob: "data/**/metadata.json",
          onFile: () => [{ col_b: "B" }],
        },
      ],
    });

    expect(await db.query("SELECT col_a FROM ta")).toHaveLength(1);
    const b = await db.query("SELECT col_b FROM tb");
    expect(b).toEqual([{ col_b: "B" }]);
  });

  it("rejects a glob placeholder that collides with a declared column", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE a (id TEXT, col_a TEXT)",
          glob: "data/{id}/metadata.json",
          onFile: () => [{ col_a: "A" }],
        },
      ],
    });

    await expect(db.ready).rejects.toThrow(/id/);
  });
});
