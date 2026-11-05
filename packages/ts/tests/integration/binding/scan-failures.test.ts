// Binding-tier test for `scanFailures`: a scan that builds has no failures,
// since any per-file `onFile` failure fails the build instead.
import { writeFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("DirSQL scanFailures", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-scan-failures-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("is empty after a clean scan", async () => {
    writeFileSync(join(dir, "a.json"), "{}");
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "*.json",
          onFile: () => [{ name: "ok" }],
        },
      ],
    });
    await db.ready;
    expect(await db.scanFailures()).toEqual([]);
  });
});
