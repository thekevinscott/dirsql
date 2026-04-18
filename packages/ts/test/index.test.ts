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

  it("creates an instance and queries data", async () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
        glob: "data/users.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
    ]);

    const rows = await db.query("SELECT * FROM users ORDER BY name");
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("Alice");
    expect(rows[0].age).toBe(30);
    expect(rows[1].name).toBe("Bob");
    expect(rows[1].age).toBe(25);
  });

  it("supports multiple tables", async () => {
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

    const users = await db.query("SELECT * FROM users ORDER BY name");
    expect(users).toHaveLength(2);

    const products = await db.query("SELECT * FROM products ORDER BY name");
    expect(products).toHaveLength(2);
    expect(products[0].name).toBe("Gadget");
    expect(products[0].price).toBeCloseTo(19.99);
  });

  it("supports glob patterns", async () => {
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

    const rows = await db.query("SELECT * FROM items ORDER BY name");
    expect(rows).toHaveLength(4);
  });

  it("supports ignore patterns", async () => {
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

    const rows = await db.query("SELECT * FROM items ORDER BY name");
    expect(rows).toHaveLength(2);
  });

  it("handles SQL queries with WHERE clauses", async () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
        glob: "data/users.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
    ]);

    const rows = await db.query("SELECT * FROM users WHERE age > 27");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("Alice");
  });

  it("handles empty directories gracefully", async () => {
    const emptyDir = mkdtempSync(join(tmpdir(), "dirsql-empty-"));
    try {
      const db = new DirSQL(emptyDir, [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: (_filePath: string, content: string) => JSON.parse(content),
        },
      ]);

      const rows = await db.query("SELECT * FROM items");
      expect(rows).toHaveLength(0);
    } finally {
      rmSync(emptyDir, { recursive: true, force: true });
    }
  });

  it("throws on invalid SQL", async () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT)",
        glob: "data/users.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
    ]);

    await expect(db.query("SELECT * FROM nonexistent")).rejects.toThrow();
  });

  it("rejects write statements via query", async () => {
    const itemDir = join(dir, "items");
    mkdirSync(itemDir, { recursive: true });
    writeFileSync(join(itemDir, "a.json"), JSON.stringify({ name: "apple" }));

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
      await expect(db.query(stmt)).rejects.toThrow(/read-only/i);
    }

    // Index is unchanged.
    const rows = await db.query("SELECT name FROM items");
    expect(rows).toEqual([{ name: "apple" }]);
  });

  it("rejects ready with invalid DDL", async () => {
    const db = new DirSQL(dir, [
      {
        ddl: "NOT VALID SQL",
        glob: "**/*.json",
        extract: () => [],
      },
    ]);
    // Construction is async: DDL errors surface via the `ready` Promise
    // rejection rather than a sync throw.
    await expect(db.ready).rejects.toThrow();
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
  it("rejects rows with extra keys when strict is true", async () => {
    writeFileSync(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", color: "red" }),
    );

    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "items/*.json",
        extract: (_filePath: string, content: string) => [JSON.parse(content)],
        strict: true,
      },
    ]);
    await expect(db.ready).rejects.toThrow();
  });

  // Docs: strict mode passes on exact key match.
  it("allows rows with exact key match when strict is true", async () => {
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

    const rows = await db.query("SELECT name, color FROM items");
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
  it("sets filePath as a relative path on watch events", async () => {
    mkdirSync(join(dir, "nested", "dir"), { recursive: true });

    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "**/*.json",
        extract: (_filePath: string, content: string) => [JSON.parse(content)],
      },
    ]);

    await db.startWatcher();

    // Give the watcher a moment to settle before writing, so the file event
    // is definitely captured.
    const relPath = join("nested", "dir", "new.json");
    writeFileSync(join(dir, relPath), JSON.stringify({ name: "relative" }));

    // Poll until we see at least one event, up to ~5s total.
    const events: RowEvent[] = [];
    const deadline = Date.now() + 5000;
    while (events.length === 0 && Date.now() < deadline) {
      events.push(...(await db.pollEvents(250)));
    }

    expect(events.length).toBeGreaterThan(0);
    const ev = events[0];
    expect(ev.filePath).toBeTruthy();
    const fp = (ev.filePath ?? "").replace(/\\/g, "/");
    // Must be relative (not absolute).
    expect(fp.startsWith("/")).toBe(false);
    expect(fp).toBe(relPath.replace(/\\/g, "/"));
  });

  // #147: pollEvents runs on the libuv threadpool, so awaiting a long poll
  // timeout does NOT starve the JS event loop. This is the watch-layer
  // analog of the async query test; a ~500ms native poll must coexist with
  // a concurrent ~50ms setTimeout.
  it("does not block the JS event loop during pollEvents", async () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "**/*.json",
        extract: (_filePath: string, content: string) => [JSON.parse(content)],
      },
    ]);

    await db.startWatcher();

    let timerFired = false;
    setTimeout(() => {
      timerFired = true;
    }, 50);

    // Native poll timeout is 10x the timer delay. With a sync poll, the
    // timer would be starved and fire only after the poll returns.
    const pollStart = Date.now();
    await db.pollEvents(500);
    const pollElapsed = Date.now() - pollStart;

    // The timer fires concurrently with the poll (it's not starved).
    expect(timerFired).toBe(true);
    // Sanity: the poll still actually parked the native thread for ~500ms.
    expect(pollElapsed).toBeGreaterThanOrEqual(400);
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
    expect(await db.query("SELECT * FROM items")).toEqual([]);
  });

  // #146: the constructor must NOT block the JS event loop. The directory
  // scan + file reads happen on the libuv threadpool; the constructor
  // returns immediately with a `ready` promise. A concurrent short setTimeout
  // should fire before or during the scan, not after it.
  it("does not block the JS event loop during construction", async () => {
    // Seed with a handful of files so the scan has real work to do.
    mkdirSync(join(dir, "items"), { recursive: true });
    for (let i = 0; i < 20; i++) {
      writeFileSync(
        join(dir, "items", `f${i}.json`),
        JSON.stringify({ name: `item-${i}` }),
      );
    }

    let timerFired = false;
    setTimeout(() => {
      timerFired = true;
    }, 1);

    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "items/*.json",
        extract: (_filePath: string, content: string) => [JSON.parse(content)],
      },
    ]);

    // The constructor returns synchronously — the scan hasn't finished yet,
    // so the timer has had a chance to fire before we await `ready`.
    await new Promise<void>((resolve) => setTimeout(resolve, 5));
    expect(timerFired).toBe(true);

    await db.ready;
    const rows = await db.query("SELECT name FROM items ORDER BY name");
    expect(rows).toHaveLength(20);
  });

  // #146: `query()` transparently awaits `ready`, so callers can issue it
  // before the initial scan has finished and it just works.
  it("query awaits ready so callers can issue it eagerly", async () => {
    writeFileSync(
      join(dir, "x.json"),
      JSON.stringify({ name: "eagerly-resolved" }),
    );

    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "*.json",
        extract: (_filePath: string, content: string) => [JSON.parse(content)],
      },
    ]);

    // Do NOT await db.ready explicitly — query() must do it internally.
    const rows = await db.query("SELECT name FROM items");
    expect(rows).toEqual([{ name: "eagerly-resolved" }]);
  });
});
