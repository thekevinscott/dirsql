// SDK e2e: an onFile value of an unsupported type is refused, not stored as
// its String() form (docs/reference/sdk.md, "Value types"). Nothing is
// mocked: real addon, real SQLite, real filesystem.

import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("onFile value types", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-e2e-on-file-value-types-"));
    await writeFile(join(dir, "a.json"), "{}\n");
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("refuses an object value with an error naming the column and type", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "t",
          ddl: "CREATE TABLE t (v)",
          glob: "a.json",
          onFile: () => [{ v: { a: 1 } }],
        },
      ],
    });
    await expect(db.ready).rejects.toThrow(
      "column 'v': unsupported value type Object",
    );
    await expect(db.query("SELECT v FROM t")).rejects.toThrow("a.json");
  });
});
