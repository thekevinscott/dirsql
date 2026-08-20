// `readFileSync` stays sync: it runs inside `onFile` callbacks, and the
// public `TableDef.onFile` signature is synchronous `(filePath) => rows[]`.
import { existsSync, readFileSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
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
          name: "users",
          ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
          glob: "data/users.json",
          onFile: (filePath: string) =>
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
          name: "users",
          ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
          glob: "data/users.json",
          onFile: (filePath: string) =>
            JSON.parse(readFileSync(filePath, "utf8")),
        },
        {
          name: "products",
          ddl: "CREATE TABLE products (name TEXT, price REAL)",
          glob: "data/products.json",
          onFile: (filePath: string) =>
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

  it("round-trips a reserved-word column", async () => {
    await writeFile(
      join(dir, "data", "orders.json"),
      JSON.stringify([{ name: "Widget", order: 7 }]),
    );
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "orders",
          ddl: 'CREATE TABLE orders (name TEXT, "order" INTEGER)',
          glob: "data/orders.json",
          onFile: (filePath: string) =>
            JSON.parse(readFileSync(filePath, "utf8")),
        },
      ],
    });

    const rows = await db.query('SELECT name, "order" FROM orders');
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("Widget");
    expect(rows[0].order).toBe(7);
  });

  it("supports glob patterns", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "data/*.json",
          onFile: (filePath: string) =>
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
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "data/*.json",
          onFile: (filePath: string) =>
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
          name: "users",
          ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
          glob: "data/users.json",
          onFile: (filePath: string) =>
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
            name: "items",
            ddl: "CREATE TABLE items (name TEXT)",
            glob: "**/*.json",
            onFile: (filePath: string) =>
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
          name: "users",
          ddl: "CREATE TABLE users (name TEXT)",
          glob: "data/users.json",
          onFile: (filePath: string) =>
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
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          onFile: (filePath: string) => [
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

    const rows = await db.query("SELECT name FROM items");
    expect(rows).toEqual([{ name: "apple" }]);
  });

  it("rejects ATTACH/DETACH via query and creates no file", async () => {
    const itemDir = join(dir, "items");
    await mkdir(itemDir, { recursive: true });
    await writeFile(join(itemDir, "a.json"), JSON.stringify({ name: "apple" }));

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          onFile: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

    const target = join(dir, "attached.db");
    await expect(db.query(`ATTACH '${target}' AS ext`)).rejects.toThrow(
      /not authorized/i,
    );
    expect(existsSync(target)).toBe(false);
    await expect(db.query("DETACH ext")).rejects.toThrow(/not authorized/i);
    // The external db a follow-up SELECT would read never gets attached.
    await expect(db.query("SELECT * FROM ext.anything")).rejects.toThrow();

    const rows = await db.query("SELECT name FROM items");
    expect(rows).toEqual([{ name: "apple" }]);
  });

  it("rejects ready with invalid DDL", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "t",
          ddl: "NOT VALID SQL",
          glob: "**/*.json",
          onFile: () => [],
        },
      ],
    });
    await expect(db.ready).rejects.toThrow();
  });
});

describe("DirSQL strict mode", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-strict-"));
    await mkdir(join(dir, "items"), { recursive: true });
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  // Docs: reference/sdk.md / reference/config.md "Strict Mode".
  it("rejects rows with extra keys when strict is true", async () => {
    await writeFile(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", color: "red" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          onFile: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
          strict: true,
        },
      ],
    });
    // Since dirsql#714 a rejected row costs its own file, not the scan:
    // the build resolves and the offending file simply contributes nothing.
    // Which file was skipped is not reachable from this binding yet (#715).
    await db.ready;
    expect(await db.query("SELECT * FROM items")).toEqual([]);
  });

  it("allows rows with exact key match when strict is true", async () => {
    await writeFile(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", color: "red" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT, color TEXT)",
          glob: "items/*.json",
          onFile: (filePath: string) => [
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

  it("rejects rows with missing keys when strict is true", async () => {
    await writeFile(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT, color TEXT)",
          glob: "items/*.json",
          onFile: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
          strict: true,
        },
      ],
    });
    // Since dirsql#714 a rejected row costs its own file, not the scan:
    // the build resolves and the offending file simply contributes nothing.
    // Which file was skipped is not reachable from this binding yet (#715).
    await db.ready;
    expect(await db.query("SELECT * FROM items")).toEqual([]);
  });
});

describe("DirSQL relaxed schema (default)", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-relaxed-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  const readJson = (filePath: string) => [
    JSON.parse(readFileSync(filePath, "utf8")),
  ];

  it("ignores keys not declared in the DDL", async () => {
    await writeFile(
      join(dir, "a.json"),
      JSON.stringify({ name: "apple", color: "red" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "*.json",
          onFile: readJson,
        },
      ],
    });

    const rows = await db.query("SELECT * FROM items");
    expect(rows).toEqual([{ name: "apple" }]);
  });

  it("fills declared-but-missing columns with NULL", async () => {
    await writeFile(join(dir, "a.json"), JSON.stringify({ name: "apple" }));

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT, color TEXT)",
          glob: "*.json",
          onFile: readJson,
        },
      ],
    });

    const rows = await db.query("SELECT * FROM items");
    expect(rows).toEqual([{ name: "apple", color: null }]);
  });
});

describe("DirSQL onFile path argument", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-onfile-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("passes the absolute path of the matched file", async () => {
    await writeFile(join(dir, "item.json"), JSON.stringify({ name: "x" }));

    const seenPaths: string[] = [];
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
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

describe("DirSQL watch events", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-watch-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  // Docs (reference/sdk.md event payloads): `filePath` is relative to the root.
  it("sets filePath as a relative path on watch events", async () => {
    await mkdir(join(dir, "nested", "dir"), { recursive: true });

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          onFile: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

    await db.startWatcher();

    const relPath = join("nested", "dir", "new.json");
    await writeFile(join(dir, relPath), JSON.stringify({ name: "relative" }));

    const events: RowEvent[] = [];
    const deadline = Date.now() + 5000;
    while (events.length === 0 && Date.now() < deadline) {
      events.push(...(await db.pollEvents(250)));
    }

    expect(events.length).toBeGreaterThan(0);
    const ev = events[0];
    expect(ev.filePath).toBeTruthy();
    const fp = (ev.filePath ?? "").replace(/\\/g, "/");
    expect(fp.startsWith("/")).toBe(false);
    expect(fp).toBe(relPath.replace(/\\/g, "/"));
  });

  it("does not block the JS event loop during pollEvents", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          onFile: (filePath: string) => [
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

    expect(timerFired).toBe(true);
    // Sanity: the poll still actually parked the native thread for ~500ms.
    expect(pollElapsed).toBeGreaterThanOrEqual(400);
  });

  it("exposes ready as an awaitable Promise", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          onFile: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

    expect(db.ready).toBeInstanceOf(Promise);
    await expect(db.ready).resolves.toBeUndefined();
    expect(await db.query("SELECT * FROM items")).toEqual([]);
  });

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
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "items/*.json",
          onFile: (filePath: string) => [
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

  it("query awaits ready so callers can issue it eagerly", async () => {
    await writeFile(
      join(dir, "x.json"),
      JSON.stringify({ name: "eagerly-resolved" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "*.json",
          onFile: (filePath: string) => [
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
      name: "users",
      ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
      glob: "data/users.json",
      onFile: (filePath: string) => JSON.parse(readFileSync(filePath, "utf8")),
    });
    expect(t).toBeInstanceOf(Table);
    expect(t.ddl).toBe("CREATE TABLE users (name TEXT, age INTEGER)");
    expect(t.glob).toBe("data/users.json");
    expect(typeof t.onFile).toBe("function");
  });

  it("instances are assignable to TableDef and accepted by DirSQL", async () => {
    const tableInstance: TableDef = new Table({
      name: "users",
      ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
      glob: "data/users.json",
      onFile: (filePath: string) => JSON.parse(readFileSync(filePath, "utf8")),
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
    const onFile = (filePath: string) =>
      JSON.parse(readFileSync(filePath, "utf8"));
    const name = "users";
    const ddl = "CREATE TABLE users (name TEXT, age INTEGER)";
    const glob = "data/users.json";

    const dbFromLiteral = new DirSQL({
      root: dir,
      tables: [{ name, ddl, glob, onFile }],
    });
    const dbFromClass = new DirSQL({
      root: dir,
      tables: [new Table({ name, ddl, glob, onFile })],
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
      name: "users",
      ddl: "CREATE TABLE users (name TEXT)",
      glob: "data/users.json",
      onFile: (filePath: string) => JSON.parse(readFileSync(filePath, "utf8")),
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
      name: "users",
      ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
      glob: "data/users.json",
      onFile: (filePath: string) => JSON.parse(readFileSync(filePath, "utf8")),
      strict: true,
    });
    expect(t.strict).toBe(true);
    const db = new DirSQL({ root: dir, tables: [t] });
    // Since dirsql#714 a rejected row costs its own file, not the scan:
    // the build resolves and the offending file simply contributes nothing.
    // Which file was skipped is not reachable from this binding yet (#715).
    await db.ready;
    expect(await db.query("SELECT * FROM users")).toEqual([]);
  });
});
