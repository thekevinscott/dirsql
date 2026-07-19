import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// Two table definitions sharing a name have no sane resolution -- last-one-wins
// silently drops a table, and an opaque SQLite `table already exists` says
// nothing about which definitions collided. Registration fails instead, naming
// the table and both sources.
describe("duplicate table names (#641)", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-641-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("rejects two programmatic tables sharing a name", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE dup (a TEXT)",
          glob: "**/*.a",
          onFile: () => [],
        },
        {
          ddl: "CREATE TABLE dup (b TEXT)",
          glob: "**/*.b",
          onFile: () => [],
        },
      ],
    });

    await expect(db.ready).rejects.toThrow(/dup/);
    await expect(db.ready).rejects.toThrow(
      /defined twice by a programmatic table/,
    );
  });

  it("rejects a programmatic table colliding with a config table", async () => {
    const config = join(dir, "dirsql.toml");
    await writeFile(
      config,
      '[[table]]\nddl = "CREATE TABLE dup (a TEXT)"\nglob = "**/*.a"\n',
    );

    const db = new DirSQL({
      root: dir,
      config,
      tables: [
        {
          ddl: "CREATE TABLE dup (b TEXT)",
          glob: "**/*.b",
          onFile: () => [],
        },
      ],
    });

    await expect(db.ready).rejects.toThrow(/dup/);
    await expect(db.ready).rejects.toThrow(/programmatic table/);
    await expect(db.ready).rejects.toThrow(
      new RegExp(config.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
    );
  });
});
