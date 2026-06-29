// Integration tests for DirSQL config serialization (#194).

import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("DirSQL serialization (toJSON / JSON.stringify)", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-serialize-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
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
    expect(json.extensions).toEqual([]);
  });

  it("reflects programmatic extensions, normalizing entrypoint to null", async () => {
    const db = new DirSQL({
      root: dir,
      extensions: [
        { path: "/ext/vec0.so", entrypoint: "sqlite3_vec_init" },
        { path: "/ext/spellfix.so" },
      ],
    });
    // toJSON is synchronous and never loads the extension, so no real
    // shared library is required here; drain ready below.
    const serialized = JSON.parse(JSON.stringify(db));
    expect(serialized.extensions).toEqual([
      { path: "/ext/vec0.so", entrypoint: "sqlite3_vec_init" },
      { path: "/ext/spellfix.so", entrypoint: null },
    ]);
    await db.ready.catch(() => {});
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
    await writeFile(
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
    await mkdir(join(dir, "data"), { recursive: true });

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

  it("merges [[dirsql.extension]] entries from .dirsql.toml, resolving relative paths", async () => {
    const cfgPath = join(dir, ".dirsql.toml");
    await writeFile(
      cfgPath,
      `[[dirsql.extension]]
path = "ext/vec0.so"
entrypoint = "sqlite3_vec_init"

[[dirsql.extension]]
path = "/abs/spellfix.so"
`,
    );
    // toJSON resolves the snapshot synchronously from the config file; the
    // extensions are never loaded here, so no real shared library is needed.
    const db = new DirSQL({ root: dir, config: cfgPath });
    const serialized = JSON.parse(JSON.stringify(db));
    expect(serialized.extensions).toEqual([
      { path: join(dir, "ext", "vec0.so"), entrypoint: "sqlite3_vec_init" },
      { path: "/abs/spellfix.so", entrypoint: null },
    ]);
    await db.ready.catch(() => {});
  });
});
