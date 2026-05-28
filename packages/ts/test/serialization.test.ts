// Integration tests for DirSQL config serialization (issue #194).
//
// A `DirSQL` instance exposes its resolved runtime state via the standard
// JS `toJSON()` hook, so `JSON.stringify(db)` produces a stable shape.
//
// The serialized form captures resolved runtime state, not construction
// parameters:
//
// - `config` (the config-file path) is excluded -- by the time the instance
//   exists the config file has been read and its contents merged into
//   `root`, `tables`, and `ignore`.
// - `extract` is excluded from the table shape -- closures are not
//   serializable.
// - `name` is excluded from the table shape.
//
// Resolution happens synchronously inside `toJSON()`, so JSON.stringify(db)
// works immediately after construction without awaiting `ready`.

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("DirSQL serialization (toJSON / JSON.stringify)", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-serialize-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  const noopExtract = () => [];

  it("exposes resolved state via JSON.stringify", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          extract: noopExtract,
        },
      ],
    });
    await db.ready;

    const json = JSON.parse(JSON.stringify(db));
    expect(json.root).toBe(dir);
    expect(Array.isArray(json.tables)).toBe(true);
    expect(json.tables).toHaveLength(1);
    expect(json.tables[0].ddl).toBe("CREATE TABLE items (name TEXT)");
    expect(json.tables[0].glob).toBe("items/*.json");
    expect(json.tables[0].strict).toBe(false);
    expect(json.ignore).toEqual([]);
    expect(json.persist).toBe(false);
    expect(json.persistPath).toBeNull();
  });

  it("uses camelCase persistPath", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          extract: noopExtract,
        },
      ],
    });
    await db.ready;

    const serialized = JSON.parse(JSON.stringify(db));
    expect(serialized).toHaveProperty("persistPath");
    expect(serialized).not.toHaveProperty("persist_path");
  });

  it("omits extract from each table", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          extract: noopExtract,
        },
      ],
    });
    await db.ready;

    const serialized = JSON.parse(JSON.stringify(db));
    for (const table of serialized.tables) {
      expect(table.extract).toBeUndefined();
    }
  });

  it("omits name from each table", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          extract: noopExtract,
        },
      ],
    });
    await db.ready;

    const serialized = JSON.parse(JSON.stringify(db));
    for (const table of serialized.tables) {
      expect(table.name).toBeUndefined();
    }
  });

  it("reflects strict: true when set on a table", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          extract: noopExtract,
          strict: true,
        },
      ],
    });
    await db.ready;

    const serialized = JSON.parse(JSON.stringify(db));
    expect(serialized.tables[0].strict).toBe(true);
  });

  it("includes ignore patterns", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          extract: noopExtract,
        },
      ],
      ignore: ["**/skip/**", "**/temp/**"],
    });
    await db.ready;

    const serialized = JSON.parse(JSON.stringify(db));
    expect(serialized.ignore).toEqual(["**/skip/**", "**/temp/**"]);
  });

  it("reflects persist and persistPath when set", async () => {
    const persistPath = join(dir, "custom-cache.db");
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          extract: noopExtract,
        },
      ],
      persist: true,
      persistPath: persistPath,
    });
    await db.ready;

    const serialized = JSON.parse(JSON.stringify(db));
    expect(serialized.persist).toBe(true);
    expect(serialized.persistPath).toBe(persistPath);
  });

  it("works before ready", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          extract: noopExtract,
        },
      ],
    });
    // Serialize before awaiting ready -- must not throw.
    const serialized = JSON.parse(JSON.stringify(db));
    expect(serialized.root).toBe(dir);
    expect(serialized.tables[0].ddl).toBe("CREATE TABLE items (name TEXT)");
    // Drain ready so a slow scan-rejection doesn't leak into other tests.
    await db.ready;
  });

  it("merges root, tables, ignore, persist from .dirsql.toml", async () => {
    const cfgPath = join(dir, ".dirsql.toml");
    writeFileSync(
      cfgPath,
      `[dirsql]
root = "data"
ignore = ["node_modules/**"]
persist = true
persist_path = "cache.db"

[[table]]
ddl = "CREATE TABLE items (_path TEXT)"
glob = "*.json"
strict = true
`,
    );
    // Create the directory the config points at so the background scan
    // succeeds and ready doesn't reject.
    mkdirSync(join(dir, "data"), { recursive: true });

    const db = new DirSQL(cfgPath);
    const serialized = JSON.parse(JSON.stringify(db));
    expect(serialized.root).toBe(join(dir, "data"));
    expect(serialized.ignore).toEqual(["node_modules/**"]);
    expect(serialized.persist).toBe(true);
    expect(serialized.persistPath).toBe(join(dir, "cache.db"));
    expect(serialized.tables).toHaveLength(1);
    expect(serialized.tables[0]).toEqual({
      ddl: "CREATE TABLE items (_path TEXT)",
      glob: "*.json",
      strict: true,
    });
    // Drain the ready promise so vitest doesn't see an unhandled rejection.
    await db.ready;
  });
});
