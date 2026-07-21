// A DirSQL constructed with no `config` and no `tables` defines no named
// tables; path-tables serve filesystem queries, and a `files` query carries a
// hint pointing at the path-table form.

import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("new DirSQL() with no config", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-default-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("defines no named tables and hints at the path-table form", async () => {
    await writeFile(join(dir, "readme.md"), "hello");
    const db = new DirSQL({ root: dir });
    await db.ready;
    await expect(db.query("SELECT basename FROM files")).rejects.toThrow(
      /no such table: files; did you mean FROM '\.\/'\?/,
    );
  });

  it("serves path-tables", async () => {
    await writeFile(join(dir, "readme.md"), "hello");
    const db = new DirSQL({ root: dir });
    await db.ready;
    const rows = await db.query("SELECT basename FROM './'");
    const names = rows.map((r) => r.basename);
    expect(names).toContain("readme.md");
  });
});
