import { readFileSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL, parseTableName } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// A quoted identifier -- the canonical shape emitted by ORMs / schema tools --
// must resolve to the bare table name (the surrounding quotes are SQL
// delimiters, not part of the name).
describe("quoted-identifier DDL (#204)", () => {
  it("parseTableName resolves to the bare identifier", () => {
    expect(parseTableName('CREATE TABLE "comments" (id TEXT)')).toBe(
      "comments",
    );
  });

  describe("end to end", () => {
    let dir: string;

    beforeEach(async () => {
      dir = await mkdtemp(join(tmpdir(), "dirsql-204-"));
      await mkdir(join(dir, "data"), { recursive: true });
      await writeFile(
        join(dir, "data", "users.json"),
        JSON.stringify([{ name: "Alice" }, { name: "Bob" }]),
      );
    });

    afterEach(async () => {
      await rm(dir, { recursive: true, force: true });
    });

    it("registers and queries a quoted-DDL table by its bare name", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            ddl: 'CREATE TABLE "users" (name TEXT)',
            glob: "data/users.json",
            // `onFile` is synchronous (returns rows, not a Promise), so the
            // file body is read with the sync API.
            onFile: (filePath: string) =>
              JSON.parse(readFileSync(filePath, "utf8")),
          },
        ],
      });

      const rows = await db.query("SELECT name FROM users ORDER BY name");
      expect(rows).toHaveLength(2);
      expect(rows[0].name).toBe("Alice");
      expect(rows[1].name).toBe("Bob");
    });
  });
});
