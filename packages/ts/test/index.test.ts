import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL, type RowEvent } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("DirSQL", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-test-"));
    mkdirSync(join(dir, "data"), { recursive: true });
    writeFileSync(
      join(dir, "data", "users.json"),
      JSON.stringify([
        { name: "Alice", age: 30 },
        { name: "Bob", age: 25 },
      ]),
    );
    writeFileSync(
      join(dir, "data", "products.json"),
      JSON.stringify([
        { name: "Widget", price: 9.99 },
        { name: "Gadget", price: 19.99 },
      ]),
    );
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("creates an instance and queries data", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
        glob: "data/users.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
    ]);

    const rows = db.query("SELECT * FROM users ORDER BY name");
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("Alice");
    expect(rows[0].age).toBe(30);
    expect(rows[1].name).toBe("Bob");
    expect(rows[1].age).toBe(25);
  });

  it("supports multiple tables", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
        glob: "data/users.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
      {
        ddl: "CREATE TABLE products (name TEXT, price REAL)",
        glob: "data/products.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
    ]);

    const users = db.query("SELECT * FROM users ORDER BY name");
    expect(users).toHaveLength(2);

    const products = db.query("SELECT * FROM products ORDER BY name");
    expect(products).toHaveLength(2);
    expect(products[0].name).toBe("Gadget");
    expect(products[0].price).toBeCloseTo(19.99);
  });

  it("supports glob patterns", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "data/*.json",
        extract: (_filePath: string, content: string) =>
          JSON.parse(content).map((item: { name: string }) => ({
            name: item.name,
          })),
      },
    ]);

    const rows = db.query("SELECT * FROM items ORDER BY name");
    expect(rows).toHaveLength(4);
  });

  it("supports ignore patterns", () => {
    const db = new DirSQL(
      dir,
      [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "data/*.json",
          extract: (_filePath: string, content: string) =>
            JSON.parse(content).map((item: { name: string }) => ({
              name: item.name,
            })),
        },
      ],
      ["data/products.json"],
    );

    const rows = db.query("SELECT * FROM items ORDER BY name");
    expect(rows).toHaveLength(2);
  });

  it("handles SQL queries with WHERE clauses", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
        glob: "data/users.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
    ]);

    const rows = db.query("SELECT * FROM users WHERE age > 27");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("Alice");
  });

  it("handles empty directories gracefully", () => {
    const emptyDir = mkdtempSync(join(tmpdir(), "dirsql-empty-"));
    try {
      const db = new DirSQL(emptyDir, [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: (_filePath: string, content: string) => JSON.parse(content),
        },
      ]);

      const rows = db.query("SELECT * FROM items");
      expect(rows).toHaveLength(0);
    } finally {
      rmSync(emptyDir, { recursive: true, force: true });
    }
  });

  it("throws on invalid SQL", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT)",
        glob: "data/users.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
    ]);

    expect(() => db.query("SELECT * FROM nonexistent")).toThrow();
  });

  it("rejects write statements via query", () => {
    const itemDir = join(dir, "items");
    mkdirSync(itemDir, { recursive: true });
    writeFileSync(
      join(itemDir, "a.json"),
      JSON.stringify({ name: "apple" }),
    );

    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "items/*.json",
        extract: (_filePath: string, content: string) => [JSON.parse(content)],
      },
    ]);

    for (const stmt of [
      "DELETE FROM items",
      "DROP TABLE items",
      "INSERT INTO items (name) VALUES ('evil')",
      "UPDATE items SET name = 'x'",
      "CREATE TABLE evil (id TEXT)",
      "ALTER TABLE items ADD COLUMN evil TEXT",
      "REPLACE INTO items (name) VALUES ('x')",
      "VACUUM",
    ]) {
      expect(() => db.query(stmt)).toThrow(/read-only/i);
    }

    // Index is unchanged.
    const rows = db.query("SELECT name FROM items");
    expect(rows).toEqual([{ name: "apple" }]);
  });

  it("throws on invalid DDL", () => {
    expect(
      () =>
        new DirSQL(dir, [
          {
            ddl: "NOT VALID SQL",
            glob: "**/*.json",
            extract: () => [],
          },
        ]),
    ).toThrow();
  });
});

// ---------------------------------------------------------------------------
// Gap-filling tests for docs features previously untested on the TS SDK side.
// Mirrors packages/python/tests/integration/test_docs_gaps.py (bead dirsql-9ng).
// See TESTS_AUDIT.md.
// ---------------------------------------------------------------------------

describe("DirSQL strict mode", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-strict-"));
    mkdirSync(join(dir, "items"), { recursive: true });
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  // Docs (guide/tables.md / guide/config.md "Strict Mode"):
  // `strict: true` on a Table def rejects rows with keys not in the DDL.
  it("rejects rows with extra keys when strict is true", () => {
    writeFileSync(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", color: "red" }),
    );

    expect(
      () =>
        new DirSQL(dir, [
          {
            ddl: "CREATE TABLE items (name TEXT)",
            glob: "items/*.json",
            extract: (_filePath: string, content: string) => [
              JSON.parse(content),
            ],
            strict: true,
          },
        ]),
    ).toThrow();
  });

  // Docs: strict mode passes on exact key match.
  it("allows rows with exact key match when strict is true", () => {
    writeFileSync(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", color: "red" }),
    );

    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT, color TEXT)",
        glob: "items/*.json",
        extract: (_filePath: string, content: string) => [JSON.parse(content)],
        strict: true,
      },
    ]);

    const rows = db.query("SELECT name, color FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("apple");
    expect(rows[0].color).toBe("red");
  });
});

describe("DirSQL watch events", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-watch-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  // Docs (guide/watching.md event payloads): `filePath` is relative to the root.
  // All examples in watching.md show relative paths (e.g. "comments/abc/index.json")
  // rather than absolute paths.
  it("sets filePath as a relative path on watch events", () => {
    mkdirSync(join(dir, "nested", "dir"), { recursive: true });

    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "**/*.json",
        extract: (_filePath: string, content: string) => [JSON.parse(content)],
      },
    ]);

    db.startWatcher();

    // Give the watcher a moment to settle before writing, so the file event
    // is definitely captured.
    const relPath = join("nested", "dir", "new.json");
    writeFileSync(join(dir, relPath), JSON.stringify({ name: "relative" }));

    // Poll until we see at least one event, up to ~5s total.
    const events: ReturnType<typeof db.pollEvents> = [];
    const deadline = Date.now() + 5000;
    while (events.length === 0 && Date.now() < deadline) {
      events.push(...db.pollEvents(250));
    }

    expect(events.length).toBeGreaterThan(0);
    const ev = events[0];
    expect(ev.filePath).toBeTruthy();
    const fp = (ev.filePath ?? "").replace(/\\/g, "/");
    // Must be relative (not absolute).
    expect(fp.startsWith("/")).toBe(false);
    expect(fp).toBe(relPath.replace(/\\/g, "/"));
  });

  // PARITY: the TS DirSQL exposes `ready: Promise<void>` and
  // `watch(): AsyncIterable<RowEvent>` to match Python/Rust.
  it("exposes ready as an awaitable Promise", async () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "**/*.json",
        extract: (_filePath: string, content: string) => [JSON.parse(content)],
      },
    ]);

    expect(db.ready).toBeInstanceOf(Promise);
    await expect(db.ready).resolves.toBeUndefined();
    // query works immediately after ready resolves.
    expect(db.query("SELECT * FROM items")).toEqual([]);
  });
});
