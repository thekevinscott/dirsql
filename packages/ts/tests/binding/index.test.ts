// `readFileSync` stays sync: it runs inside `extract` callbacks, and the
// public `TableDef.extract` signature is synchronous `(filePath) => rows[]`.
// Only the test's own setup/teardown plumbing moves to `node:fs/promises`.
import { readFileSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL, type RowEvent, Table, type TableDef } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("DirSQL", () => {
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
    await writeFile(
      join(dir, "data", "products.json"),
      JSON.stringify([
        { name: "Widget", price: 9.99 },
        { name: "Gadget", price: 19.99 },
      ]),
    );
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("creates an instance and queries data", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
          glob: "data/users.json",
          extract: (filePath: string) =>
            JSON.parse(readFileSync(filePath, "utf8")),
        },
      ],
    });

    const rows = await db.query("SELECT * FROM users ORDER BY name");
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("Alice");
    expect(rows[0].age).toBe(30);
    expect(rows[1].name).toBe("Bob");
    expect(rows[1].age).toBe(25);
  });

  it("supports multiple tables", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
          glob: "data/users.json",
          extract: (filePath: string) =>
            JSON.parse(readFileSync(filePath, "utf8")),
        },
        {
          ddl: "CREATE TABLE products (name TEXT, price REAL)",
          glob: "data/products.json",
          extract: (filePath: string) =>
            JSON.parse(readFileSync(filePath, "utf8")),
        },
      ],
    });

    const users = await db.query("SELECT * FROM users ORDER BY name");
    expect(users).toHaveLength(2);

    const products = await db.query("SELECT * FROM products ORDER BY name");
    expect(products).toHaveLength(2);
    expect(products[0].name).toBe("Gadget");
    expect(products[0].price).toBeCloseTo(19.99);
  });

  it("supports glob patterns", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "data/*.json",
          extract: (filePath: string) =>
            JSON.parse(readFileSync(filePath, "utf8")).map(
              (item: { name: string }) => ({
                name: item.name,
              }),
            ),
        },
      ],
    });

    const rows = await db.query("SELECT * FROM items ORDER BY name");
    expect(rows).toHaveLength(4);
  });

  it("supports ignore patterns", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "data/*.json",
          extract: (filePath: string) =>
            JSON.parse(readFileSync(filePath, "utf8")).map(
              (item: { name: string }) => ({
                name: item.name,
              }),
            ),
        },
      ],
      ignore: ["data/products.json"],
    });

    const rows = await db.query("SELECT * FROM items ORDER BY name");
    expect(rows).toHaveLength(2);
  });

  it("handles SQL queries with WHERE clauses", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
          glob: "data/users.json",
          extract: (filePath: string) =>
            JSON.parse(readFileSync(filePath, "utf8")),
        },
      ],
    });

    const rows = await db.query("SELECT * FROM users WHERE age > 27");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("Alice");
  });

  it("handles empty directories gracefully", async () => {
    const emptyDir = await mkdtemp(join(tmpdir(), "dirsql-empty-"));
    try {
      const db = new DirSQL({
        root: emptyDir,
        tables: [
          {
            ddl: "CREATE TABLE items (name TEXT)",
            glob: "**/*.json",
            extract: (filePath: string) =>
              JSON.parse(readFileSync(filePath, "utf8")),
          },
        ],
      });

      const rows = await db.query("SELECT * FROM items");
      expect(rows).toHaveLength(0);
    } finally {
      await rm(emptyDir, { recursive: true, force: true });
    }
  });

  it("throws on invalid SQL", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE users (name TEXT)",
          glob: "data/users.json",
          extract: (filePath: string) =>
            JSON.parse(readFileSync(filePath, "utf8")),
        },
      ],
    });

    await expect(db.query("SELECT * FROM nonexistent")).rejects.toThrow();
  });

  it("rejects write statements via query", async () => {
    const itemDir = join(dir, "items");
    await mkdir(itemDir, { recursive: true });
    await writeFile(join(itemDir, "a.json"), JSON.stringify({ name: "apple" }));

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          extract: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

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
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "NOT VALID SQL",
          glob: "**/*.json",
          extract: () => [],
        },
      ],
    });
    // Construction is async: DDL errors surface via the `ready` Promise
    // rejection rather than a sync throw.
    await expect(db.ready).rejects.toThrow();
  });
});

// ---------------------------------------------------------------------------
// Gap-filling tests for docs features previously untested on the TS SDK side.
// Mirrors packages/python/tests/binding/docs_gaps_test.py (bead dirsql-9ng).
// See TESTS_AUDIT.md.
// ---------------------------------------------------------------------------

describe("DirSQL strict mode", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-strict-"));
    await mkdir(join(dir, "items"), { recursive: true });
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  // Docs (guide/tables.md / guide/config.md "Strict Mode"):
  // `strict: true` on a Table def rejects rows with keys not in the DDL.
  it("rejects rows with extra keys when strict is true", async () => {
    await writeFile(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", color: "red" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          extract: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
          strict: true,
        },
      ],
    });
    await expect(db.ready).rejects.toThrow();
  });

  // Docs: strict mode passes on exact key match.
  it("allows rows with exact key match when strict is true", async () => {
    await writeFile(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", color: "red" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT, color TEXT)",
          glob: "items/*.json",
          extract: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
          strict: true,
        },
      ],
    });

    const rows = await db.query("SELECT name, color FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("apple");
    expect(rows[0].color).toBe("red");
  });
});

describe("DirSQL watch events", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-watch-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  // Docs (guide/watching.md event payloads): `filePath` is relative to the root.
  // All examples in watching.md show relative paths (e.g. "comments/abc/index.json")
  // rather than absolute paths.
  it("sets filePath as a relative path on watch events", async () => {
    await mkdir(join(dir, "nested", "dir"), { recursive: true });

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

    await db.startWatcher();

    // Give the watcher a moment to settle before writing, so the file event
    // is definitely captured.
    const relPath = join("nested", "dir", "new.json");
    await writeFile(join(dir, relPath), JSON.stringify({ name: "relative" }));

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
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

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
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

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
    await mkdir(join(dir, "items"), { recursive: true });
    for (let i = 0; i < 20; i++) {
      await writeFile(
        join(dir, "items", `f${i}.json`),
        JSON.stringify({ name: `item-${i}` }),
      );
    }

    let timerFired = false;
    setTimeout(() => {
      timerFired = true;
    }, 1);

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          extract: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

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
    await writeFile(
      join(dir, "x.json"),
      JSON.stringify({ name: "eagerly-resolved" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "*.json",
          extract: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

    // Do NOT await db.ready explicitly — query() must do it internally.
    const rows = await db.query("SELECT name FROM items");
    expect(rows).toEqual([{ name: "eagerly-resolved" }]);
  });
});

// ---------------------------------------------------------------------------
// Parity-restoring: `Table` class export (#216).
// Python has `Table(ddl=..., glob=..., extract=...)` and Rust has
// `Table::new(...)`; TS used to require plain object literals only. The new
// `Table` class is a thin identity wrapper around `TableDef` -- constructing
// `new Table({...})` produces something structurally identical to the literal,
// and anything accepting `TableDef[]` must accept both forms interchangeably.
// ---------------------------------------------------------------------------

describe("Table class", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-table-class-"));
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

  it("is importable as a constructable class", () => {
    const t = new Table({
      ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
      glob: "data/users.json",
      extract: (filePath: string) => JSON.parse(readFileSync(filePath, "utf8")),
    });
    expect(t).toBeInstanceOf(Table);
    expect(t.ddl).toBe("CREATE TABLE users (name TEXT, age INTEGER)");
    expect(t.glob).toBe("data/users.json");
    expect(typeof t.extract).toBe("function");
  });

  it("instances are assignable to TableDef and accepted by DirSQL", async () => {
    const tableInstance: TableDef = new Table({
      ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
      glob: "data/users.json",
      extract: (filePath: string) => JSON.parse(readFileSync(filePath, "utf8")),
    });

    const db = new DirSQL({
      root: dir,
      tables: [tableInstance],
    });

    const rows = await db.query("SELECT * FROM users ORDER BY name");
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("Alice");
    expect(rows[1].name).toBe("Bob");
  });

  it("plain object and Table instance produce identical query behavior", async () => {
    const extract = (filePath: string) =>
      JSON.parse(readFileSync(filePath, "utf8"));
    const ddl = "CREATE TABLE users (name TEXT, age INTEGER)";
    const glob = "data/users.json";

    const dbFromLiteral = new DirSQL({
      root: dir,
      tables: [{ ddl, glob, extract }],
    });
    const dbFromClass = new DirSQL({
      root: dir,
      tables: [new Table({ ddl, glob, extract })],
    });

    const literalRows = await dbFromLiteral.query(
      "SELECT * FROM users ORDER BY name",
    );
    const classRows = await dbFromClass.query(
      "SELECT * FROM users ORDER BY name",
    );
    expect(classRows).toEqual(literalRows);
  });

  it("has the same enumerable keys as the equivalent plain object literal", () => {
    const def = {
      ddl: "CREATE TABLE users (name TEXT)",
      glob: "data/users.json",
      extract: (filePath: string) => JSON.parse(readFileSync(filePath, "utf8")),
    };
    const t = new Table(def);
    expect(Object.keys(t).sort()).toEqual(Object.keys(def).sort());
  });

  it("propagates the optional `strict` flag", async () => {
    await writeFile(
      join(dir, "data", "users.json"),
      JSON.stringify([{ name: "Alice", age: 30, extra: "nope" }]),
    );
    const t = new Table({
      ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
      glob: "data/users.json",
      extract: (filePath: string) => JSON.parse(readFileSync(filePath, "utf8")),
      strict: true,
    });
    expect(t.strict).toBe(true);
    const db = new DirSQL({ root: dir, tables: [t] });
    await expect(db.ready).rejects.toThrow();
  });
});
