import { readFileSync } from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

let dir: string;

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), "dirsql-gaps-"));
});

afterEach(async () => {
  await rm(dir, { recursive: true, force: true });
});

// Docs (reference/sdk.md / reference/config.md "Strict Mode"): the default
// (relaxed) mode drops onFile keys that aren't declared in the DDL and
// fills declared-but-missing columns with NULL.
describe("DirSQL relaxed schema (default)", () => {
  it("ignores extra keys by default", async () => {
    await writeFile(
      join(dir, "a.json"),
      JSON.stringify({ name: "apple", color: "red" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "*.json",
          onFile: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

    const rows = await db.query("SELECT * FROM items");
    expect(rows).toEqual([{ name: "apple" }]);
  });

  it("fills missing keys with NULL", async () => {
    await writeFile(join(dir, "a.json"), JSON.stringify({ name: "apple" }));

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT, color TEXT)",
          glob: "*.json",
          onFile: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

    const rows = await db.query("SELECT * FROM items");
    expect(rows).toEqual([{ name: "apple", color: null }]);
  });
});

// Docs: strict mode errors on declared columns the onFile row is missing.
describe("DirSQL strict mode (missing keys)", () => {
  it("rejects rows with missing keys when strict is true", async () => {
    await writeFile(join(dir, "a.json"), JSON.stringify({ name: "apple" }));

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT, color TEXT)",
          glob: "*.json",
          onFile: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
          strict: true,
        },
      ],
    });

    await expect(db.ready).rejects.toThrow();
  });
});

// Docs (reference/sdk.md "Supported value types"): `Buffer` / `Uint8Array`
// -> SQLite BLOB; BLOB columns come back from `query()` as `Buffer`.
describe("DirSQL Buffer -> BLOB", () => {
  const payload = Buffer.from([0x00, 0x01, 0x02, 0xff, 0xfe]);

  async function roundTrip(data: unknown): Promise<unknown> {
    await writeFile(join(dir, "marker.json"), "{}");
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE blobs (name TEXT, data BLOB)",
          glob: "*.json",
          onFile: () => [{ name: "bin", data }],
        },
      ],
    });
    const rows = await db.query("SELECT * FROM blobs");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("bin");
    return rows[0].data;
  }

  it("round-trips a Buffer through a BLOB column", async () => {
    const data = await roundTrip(payload);
    expect(Buffer.isBuffer(data)).toBe(true);
    expect(Buffer.compare(data as Buffer, payload)).toBe(0);
  });

  it("maps a Uint8Array to a BLOB and returns it as a Buffer", async () => {
    const data = await roundTrip(new Uint8Array(payload));
    expect(Buffer.isBuffer(data)).toBe(true);
    expect(Buffer.compare(data as Buffer, payload)).toBe(0);
  });
});

// Docs (reference/sdk.md "onFile"): the callback receives the filesystem
// path of the matched file (absolute when the root is absolute); dirsql
// does not read file contents itself.
describe("DirSQL onFile path argument", () => {
  it("passes the absolute path of the matched file", async () => {
    await writeFile(join(dir, "item.json"), JSON.stringify({ name: "x" }));

    const seenPaths: string[] = [];
    const db = new DirSQL({
      root: dir, // mkdtemp returns an absolute path
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "*.json",
          onFile: (filePath: string) => {
            seenPaths.push(filePath);
            return [JSON.parse(readFileSync(filePath, "utf8"))];
          },
        },
      ],
    });
    await db.ready;

    expect(seenPaths).toHaveLength(1);
    expect(isAbsolute(seenPaths[0])).toBe(true);
    expect(seenPaths[0].endsWith("item.json")).toBe(true);
  });
});
