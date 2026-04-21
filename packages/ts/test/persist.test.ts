import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("DirSQL persist", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-persist-"));
    mkdirSync(join(dir, "items"), { recursive: true });
    writeFileSync(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", price: 1.5 }),
    );
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  function makeTable(box: { count: number }) {
    return {
      ddl: "CREATE TABLE items (name TEXT, price REAL)",
      glob: "items/*.json",
      extract: (_filePath: string, content: string) => {
        box.count += 1;
        return [JSON.parse(content)];
      },
    };
  }

  it("writes the cache db to .dirsql/cache.db on cold start", async () => {
    const box = { count: 0 };
    const db = new DirSQL({
      root: dir,
      tables: [makeTable(box)],
      persist: true,
    });
    const rows = await db.query("SELECT * FROM items");
    expect(rows).toHaveLength(1);
    expect(existsSync(join(dir, ".dirsql", "cache.db"))).toBe(true);
  });

  it("trusts unchanged files on warm start", async () => {
    const box1 = { count: 0 };
    const db1 = new DirSQL({
      root: dir,
      tables: [makeTable(box1)],
      persist: true,
    });
    await db1.ready;
    expect(box1.count).toBe(1);

    const box2 = { count: 0 };
    const db2 = new DirSQL({
      root: dir,
      tables: [makeTable(box2)],
      persist: true,
    });
    await db2.ready;
    expect(box2.count).toBe(0);
    const rows = await db2.query("SELECT * FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("apple");
  });

  it("re-parses changed files", async () => {
    const box1 = { count: 0 };
    const db1 = new DirSQL({
      root: dir,
      tables: [makeTable(box1)],
      persist: true,
    });
    await db1.ready;

    // Bump mtime far enough into the future to escape the racy window.
    await new Promise((r) => setTimeout(r, 50));
    writeFileSync(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "cherry", price: 9.99 }),
    );
    const future = new Date(Date.now() + 5000);
    utimesSync(join(dir, "items", "a.json"), future, future);

    const box2 = { count: 0 };
    const db2 = new DirSQL({
      root: dir,
      tables: [makeTable(box2)],
      persist: true,
    });
    await db2.ready;
    expect(box2.count).toBe(1);
    const rows = await db2.query("SELECT * FROM items");
    expect(rows[0].name).toBe("cherry");
  });

  it("drops rows for files removed between runs", async () => {
    writeFileSync(
      join(dir, "items", "b.json"),
      JSON.stringify({ name: "banana", price: 0.75 }),
    );

    const box1 = { count: 0 };
    const db1 = new DirSQL({
      root: dir,
      tables: [makeTable(box1)],
      persist: true,
    });
    await db1.ready;

    rmSync(join(dir, "items", "b.json"));

    const box2 = { count: 0 };
    const db2 = new DirSQL({
      root: dir,
      tables: [makeTable(box2)],
      persist: true,
    });
    await db2.ready;
    const rows = await db2.query("SELECT name FROM items ORDER BY name");
    expect(rows.map((r) => r.name)).toEqual(["apple"]);
  });

  it("ingests files added between runs", async () => {
    const box1 = { count: 0 };
    const db1 = new DirSQL({
      root: dir,
      tables: [makeTable(box1)],
      persist: true,
    });
    await db1.ready;

    writeFileSync(
      join(dir, "items", "b.json"),
      JSON.stringify({ name: "banana", price: 0.75 }),
    );

    const box2 = { count: 0 };
    const db2 = new DirSQL({
      root: dir,
      tables: [makeTable(box2)],
      persist: true,
    });
    await db2.ready;
    expect(box2.count).toBe(1);
    const rows = await db2.query("SELECT name FROM items ORDER BY name");
    expect(rows.map((r) => r.name)).toEqual(["apple", "banana"]);
  });

  it("forces a full rebuild when the DDL changes", async () => {
    const box1 = { count: 0 };
    const db1 = new DirSQL({
      root: dir,
      tables: [makeTable(box1)],
      persist: true,
    });
    await db1.ready;

    const box2 = { count: 0 };
    const db2 = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT, price REAL, sku TEXT)",
          glob: "items/*.json",
          extract: (_filePath: string, content: string) => {
            box2.count += 1;
            return [{ ...JSON.parse(content), sku: "X" }];
          },
        },
      ],
      persist: true,
    });
    await db2.ready;
    expect(box2.count).toBe(1);
    const rows = await db2.query("SELECT * FROM items");
    expect(rows[0].sku).toBe("X");
  });

  it("never indexes files inside the .dirsql directory", async () => {
    mkdirSync(join(dir, ".dirsql", "items"), { recursive: true });
    writeFileSync(
      join(dir, ".dirsql", "items", "boom.json"),
      JSON.stringify({ name: "BOOM", price: -1 }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT, price REAL)",
          glob: "**/*.json",
          extract: (_filePath: string, content: string) => [JSON.parse(content)],
        },
      ],
      persist: true,
    });
    await db.ready;
    const rows = await db.query("SELECT name FROM items");
    expect(rows.map((r) => r.name)).toEqual(["apple"]);
  });

  it("honors a custom persistPath", async () => {
    const custom = join(dir, "elsewhere", "my-cache.sqlite");
    const box = { count: 0 };
    const db = new DirSQL({
      root: dir,
      tables: [makeTable(box)],
      persist: true,
      persistPath: custom,
    });
    await db.ready;
    expect(existsSync(custom)).toBe(true);
    expect(existsSync(join(dir, ".dirsql", "cache.db"))).toBe(false);
  });
});
