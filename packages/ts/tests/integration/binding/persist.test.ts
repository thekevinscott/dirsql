// `readFileSync` stays sync: it runs inside `onFile` callbacks, whose public
// signature is synchronous.
import { readFileSync } from "node:fs";
import {
  mkdir,
  mkdtemp,
  readFile,
  rm,
  unlink,
  utimes,
  writeFile,
} from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { DirSQL } from "dirsql";
import initSqlJs from "sql.js";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { exists } from "../../exists.js";

// Some tests corrupt dirsql's on-disk cache (`.dirsql/cache.db`) out-of-band
// to exercise the racy-window and dirsql_version-bump reconcile paths. sql.js
// (WASM SQLite) is used instead of `node:sqlite`, which only exists on Node
// 22.5+. sql.js is in-memory, so we read the cache bytes, mutate, and write
// them back. The cache uses WAL mode, so after closing the Rust connection,
// we must checkpoint the WAL to ensure the main db file is complete.
const resolveModule = createRequire(import.meta.url).resolve;
const sqlJsReady = initSqlJs({
  locateFile: (file) => join(dirname(resolveModule("sql.js")), file),
});

/** Open `.dirsql/cache.db` with sql.js, run `sql`, write the bytes back. */
async function corruptCache(cachePath: string, sql: string): Promise<void> {
  const SQL = await sqlJsReady;
  // With WAL mode, the cache has sidecar files (cache.db-wal, cache.db-shm).
  // sql.js reads the raw binary file and doesn't understand WAL mode.
  // Delete the sidecar files so sql.js reads a clean database file.
  try {
    await unlink(cachePath + "-wal");
  } catch {
    // File might not exist if WAL hasn't created sidecar yet
  }
  try {
    await unlink(cachePath + "-shm");
  } catch {
    // File might not exist if WAL hasn't created sidecar yet
  }

  const db = new SQL.Database(await readFile(cachePath));
  try {
    db.run(sql);
    await writeFile(cachePath, db.export());
  } finally {
    db.close();
  }
}

describe("DirSQL persist", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-persist-"));
    await mkdir(join(dir, "items"), { recursive: true });
    await writeFile(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", price: 1.5 }),
    );
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  function makeTable(box: { count: number }) {
    return {
      ddl: "CREATE TABLE items (name TEXT, price REAL)",
      glob: "items/*.json",
      onFile: (filePath: string) => {
        box.count += 1;
        return [JSON.parse(readFileSync(filePath, "utf8"))];
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
    expect(await exists(join(dir, ".dirsql", "cache.db"))).toBe(true);
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
    await writeFile(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "cherry", price: 9.99 }),
    );
    const future = new Date(Date.now() + 5000);
    await utimes(join(dir, "items", "a.json"), future, future);

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
    await writeFile(
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

    await rm(join(dir, "items", "b.json"));

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

    await writeFile(
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
          onFile: (filePath: string) => {
            box2.count += 1;
            return [
              { ...JSON.parse(readFileSync(filePath, "utf8")), sku: "X" },
            ];
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
    await mkdir(join(dir, ".dirsql", "items"), { recursive: true });
    await writeFile(
      join(dir, ".dirsql", "items", "boom.json"),
      JSON.stringify({ name: "BOOM", price: -1 }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT, price REAL)",
          glob: "**/*.json",
          onFile: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
      persist: true,
    });
    await db.ready;
    const rows = await db.query("SELECT name FROM items");
    expect(rows.map((r) => r.name)).toEqual(["apple"]);
  });

  it("hash-confirms files that fall inside the racy window", async () => {
    // Files whose cached mtime >= snapshot_ns are considered "racy" and must
    // be hash-confirmed instead of trusted. Corrupt the cached hash so the
    // hash check fails; the file must then be re-parsed.
    const box1 = { count: 0 };
    const db1 = new DirSQL({
      root: dir,
      tables: [makeTable(box1)],
      persist: true,
    });
    await db1.ready;
    expect(box1.count).toBe(1);
    // Explicitly close the connection to trigger WAL checkpoint.
    // napi close() consumes self, so this is the only call on this instance.
    db1.close();
    // Wait for WAL checkpoint to complete before reading with sql.js.
    await new Promise((resolve) => setTimeout(resolve, 50));

    const cache = join(dir, ".dirsql", "cache.db");
    await corruptCache(
      cache,
      "UPDATE _dirsql_files SET snapshot_ns = 0, content_hash = zeroblob(32)",
    );

    const box2 = { count: 0 };
    const db2 = new DirSQL({
      root: dir,
      tables: [makeTable(box2)],
      persist: true,
    });
    await db2.ready;
    expect(box2.count).toBe(1);
    const rows = await db2.query("SELECT name FROM items");
    expect(rows[0].name).toBe("apple");
  });

  it("rebuilds the cache when the dirsql_version meta changes", async () => {
    const box1 = { count: 0 };
    const db1 = new DirSQL({
      root: dir,
      tables: [makeTable(box1)],
      persist: true,
    });
    await db1.ready;
    expect(box1.count).toBe(1);
    // Explicitly close the connection to trigger WAL checkpoint.
    // napi close() consumes self, so this is the only call on this instance.
    db1.close();
    // Wait for WAL checkpoint to complete before reading with sql.js.
    await new Promise((resolve) => setTimeout(resolve, 50));

    const cache = join(dir, ".dirsql", "cache.db");
    await corruptCache(
      cache,
      "UPDATE _dirsql_meta SET value = 'bogus-version' WHERE key = 'dirsql_version'",
    );

    const box2 = { count: 0 };
    const db2 = new DirSQL({
      root: dir,
      tables: [makeTable(box2)],
      persist: true,
    });
    await db2.ready;
    expect(box2.count).toBe(1);
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
    expect(await exists(custom)).toBe(true);
    expect(await exists(join(dir, ".dirsql", "cache.db"))).toBe(false);
  });
});
