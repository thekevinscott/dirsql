// Binding-tier red tests for #570 (real napi core, real fs, nothing mocked):
// the per-file row seam is `onFile`, not `extract`. The old `extract`
// spelling is a hard break (no deprecation alias).
import { readFileSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("the onFile table seam against the real core", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-test-"));
    await mkdir(join(dir, "data"), { recursive: true });
    await writeFile(
      join(dir, "data", "users.json"),
      JSON.stringify([
        { name: "Alice", age: 30 },
        { name: "Bob", age: 25 },
      ]),
    );
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("indexes rows produced by a table's onFile callback", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "users",
          ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
          glob: "data/users.json",
          onFile: (filePath: string) =>
            JSON.parse(readFileSync(filePath, "utf8")),
        },
      ],
    });

    const rows = await db.query("SELECT name FROM users ORDER BY name");
    expect(rows.map((r) => r.name)).toEqual(["Alice", "Bob"]);
  });
});
