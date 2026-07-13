// #603: a DirSQL constructed with no `config` and no `tables` serves the
// baked-in default `files` table (parity with the CLI's no-`-c` default),
// not an empty index.

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

  it("serves the baked-in files table", async () => {
    await writeFile(join(dir, "readme.md"), "hello");
    const db = new DirSQL({ root: dir });
    await db.ready;
    const rows = await db.query("SELECT basename FROM files");
    const names = rows.map((r) => r.basename);
    expect(names).toContain("readme.md");
  });
});
