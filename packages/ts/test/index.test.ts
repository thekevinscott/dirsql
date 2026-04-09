import { describe, it, expect, beforeEach, afterEach } from "vitest";
import {
  mkdtempSync,
  writeFileSync,
  rmSync,
  mkdirSync,
  unlinkSync,
} from "fs";
import { join } from "path";
import { tmpdir } from "os";
import { DirSQL } from "../index.js";

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

  it("excludes internal tracking columns from results", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
        glob: "data/users.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
    ]);

    const rows = db.query("SELECT * FROM users LIMIT 1");
    expect(rows).toHaveLength(1);
    expect(rows[0]).not.toHaveProperty("_dirsql_file_path");
    expect(rows[0]).not.toHaveProperty("_dirsql_row_index");
  });
});

describe("DirSQL relaxed schema", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-schema-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("ignores extra keys by default", () => {
    writeFileSync(
      join(dir, "item.json"),
      JSON.stringify([{ name: "apple", color: "red", weight: 150 }]),
    );

    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "*.json",
        extract: (_: string, content: string) => JSON.parse(content),
      },
    ]);

    const rows = db.query("SELECT * FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("apple");
    expect(rows[0]).not.toHaveProperty("color");
    expect(rows[0]).not.toHaveProperty("weight");
  });

  it("fills missing keys with null", () => {
    writeFileSync(
      join(dir, "item.json"),
      JSON.stringify([{ name: "apple" }]),
    );

    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT, color TEXT, count INTEGER)",
        glob: "*.json",
        extract: (_: string, content: string) => JSON.parse(content),
      },
    ]);

    const rows = db.query("SELECT * FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("apple");
    expect(rows[0].color).toBeNull();
    expect(rows[0].count).toBeNull();
  });

  it("rejects extra keys in strict mode", () => {
    writeFileSync(
      join(dir, "item.json"),
      JSON.stringify([{ name: "apple", color: "red" }]),
    );

    expect(
      () =>
        new DirSQL(dir, [
          {
            ddl: "CREATE TABLE items (name TEXT)",
            glob: "*.json",
            extract: (_: string, content: string) => JSON.parse(content),
            strict: true,
          },
        ]),
    ).toThrow();
  });

  it("rejects missing keys in strict mode", () => {
    writeFileSync(
      join(dir, "item.json"),
      JSON.stringify([{ name: "apple" }]),
    );

    expect(
      () =>
        new DirSQL(dir, [
          {
            ddl: "CREATE TABLE items (name TEXT, color TEXT)",
            glob: "*.json",
            extract: (_: string, content: string) => JSON.parse(content),
            strict: true,
          },
        ]),
    ).toThrow();
  });

  it("allows exact match in strict mode", () => {
    writeFileSync(
      join(dir, "item.json"),
      JSON.stringify([{ name: "apple", color: "red" }]),
    );

    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT, color TEXT)",
        glob: "*.json",
        extract: (_: string, content: string) => JSON.parse(content),
        strict: true,
      },
    ]);

    const rows = db.query("SELECT * FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("apple");
    expect(rows[0].color).toBe("red");
  });
});

describe("DirSQL.fromConfig", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-config-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("loads JSON files via config", () => {
    mkdirSync(join(dir, "items"), { recursive: true });
    writeFileSync(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", price: 1.5 }),
    );
    writeFileSync(
      join(dir, "items", "b.json"),
      JSON.stringify({ name: "banana", price: 0.75 }),
    );
    writeFileSync(
      join(dir, ".dirsql.toml"),
      `[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "items/*.json"
`,
    );

    const db = DirSQL.fromConfig(join(dir, ".dirsql.toml"));
    const rows = db.query("SELECT * FROM items ORDER BY name");
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("apple");
    expect(rows[0].price).toBeCloseTo(1.5);
    expect(rows[1].name).toBe("banana");
  });

  it("loads CSV files via config", () => {
    writeFileSync(join(dir, "data.csv"), "name,count\napples,10\noranges,20\n");
    writeFileSync(
      join(dir, ".dirsql.toml"),
      `[[table]]
ddl = "CREATE TABLE produce (name TEXT, count TEXT)"
glob = "*.csv"
`,
    );

    const db = DirSQL.fromConfig(join(dir, ".dirsql.toml"));
    const rows = db.query("SELECT * FROM produce ORDER BY name");
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("apples");
  });

  it("loads JSONL files via config", () => {
    writeFileSync(
      join(dir, "events.jsonl"),
      JSON.stringify({ type: "click", count: 5 }) +
        "\n" +
        JSON.stringify({ type: "view", count: 100 }) +
        "\n",
    );
    writeFileSync(
      join(dir, ".dirsql.toml"),
      `[[table]]
ddl = "CREATE TABLE events (type TEXT, count INTEGER)"
glob = "*.jsonl"
`,
    );

    const db = DirSQL.fromConfig(join(dir, ".dirsql.toml"));
    const rows = db.query("SELECT * FROM events ORDER BY type");
    expect(rows).toHaveLength(2);
    expect(rows[0].type).toBe("click");
    expect(rows[0].count).toBe(5);
  });

  it("respects ignore patterns from config", () => {
    mkdirSync(join(dir, "data"), { recursive: true });
    mkdirSync(join(dir, "data", "node_modules"), { recursive: true });
    writeFileSync(
      join(dir, "data", "good.json"),
      JSON.stringify({ val: 1 }),
    );
    writeFileSync(
      join(dir, "data", "node_modules", "bad.json"),
      JSON.stringify({ val: 2 }),
    );
    writeFileSync(
      join(dir, ".dirsql.toml"),
      `[dirsql]
ignore = ["**/node_modules/**"]

[[table]]
ddl = "CREATE TABLE items (val INTEGER)"
glob = "data/**/*.json"
`,
    );

    const db = DirSQL.fromConfig(join(dir, ".dirsql.toml"));
    const rows = db.query("SELECT * FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].val).toBe(1);
  });

  it("loads multiple tables from config", () => {
    mkdirSync(join(dir, "posts"), { recursive: true });
    mkdirSync(join(dir, "authors"), { recursive: true });
    writeFileSync(
      join(dir, "posts", "hello.json"),
      JSON.stringify({ title: "Hello" }),
    );
    writeFileSync(
      join(dir, "authors", "alice.json"),
      JSON.stringify({ name: "Alice" }),
    );
    writeFileSync(
      join(dir, ".dirsql.toml"),
      `[[table]]
ddl = "CREATE TABLE posts (title TEXT)"
glob = "posts/*.json"

[[table]]
ddl = "CREATE TABLE authors (name TEXT)"
glob = "authors/*.json"
`,
    );

    const db = DirSQL.fromConfig(join(dir, ".dirsql.toml"));
    const posts = db.query("SELECT * FROM posts");
    const authors = db.query("SELECT * FROM authors");
    expect(posts).toHaveLength(1);
    expect(authors).toHaveLength(1);
    expect(posts[0].title).toBe("Hello");
    expect(authors[0].name).toBe("Alice");
  });

  it("throws on missing config file", () => {
    expect(() => DirSQL.fromConfig(join(dir, "nonexistent.toml"))).toThrow();
  });

  it("throws on invalid TOML", () => {
    writeFileSync(join(dir, ".dirsql.toml"), "this is not valid [[[");
    expect(() => DirSQL.fromConfig(join(dir, ".dirsql.toml"))).toThrow();
  });

  it("throws on unsupported format", () => {
    writeFileSync(join(dir, "data.dat"), "some data");
    writeFileSync(
      join(dir, ".dirsql.toml"),
      `[[table]]
ddl = "CREATE TABLE t (x TEXT)"
glob = "*.dat"
`,
    );

    expect(() => DirSQL.fromConfig(join(dir, ".dirsql.toml"))).toThrow(
      /[Ff]ormat|[Uu]nsupported/,
    );
  });
});

describe("DirSQL watch", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-watch-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("emits insert events for new files", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "**/*.json",
        extract: (_: string, content: string) => JSON.parse(content),
      },
    ]);

    db.startWatcher();

    // Small delay for watcher to initialize
    const start = Date.now();
    while (Date.now() - start < 300) {
      /* busy wait */
    }

    writeFileSync(
      join(dir, "new_item.json"),
      JSON.stringify([{ name: "apple" }]),
    );

    // Poll for events with retry
    let events: ReturnType<typeof db.pollEvents> = [];
    const deadline = Date.now() + 5000;
    while (events.length === 0 && Date.now() < deadline) {
      events = db.pollEvents(500);
    }

    expect(events.length).toBeGreaterThanOrEqual(1);
    expect(events[0].action).toBe("insert");
    expect(events[0].table).toBe("items");
    expect(events[0].row?.name).toBe("apple");
  });

  it("emits delete events for removed files", () => {
    writeFileSync(
      join(dir, "doomed.json"),
      JSON.stringify([{ name: "doomed" }]),
    );

    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "**/*.json",
        extract: (_: string, content: string) => JSON.parse(content),
      },
    ]);

    const initial = db.query("SELECT * FROM items");
    expect(initial).toHaveLength(1);

    db.startWatcher();

    const start = Date.now();
    while (Date.now() - start < 300) {
      /* busy wait */
    }

    unlinkSync(join(dir, "doomed.json"));

    let events: ReturnType<typeof db.pollEvents> = [];
    const deadline = Date.now() + 5000;
    while (events.length === 0 && Date.now() < deadline) {
      events = db.pollEvents(500);
    }

    expect(events.length).toBeGreaterThanOrEqual(1);
    expect(events[0].action).toBe("delete");
    expect(events[0].table).toBe("items");
    expect(events[0].row?.name).toBe("doomed");
  });

  it("emits update events for modified files", () => {
    writeFileSync(
      join(dir, "item.json"),
      JSON.stringify([{ name: "draft" }]),
    );

    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "**/*.json",
        extract: (_: string, content: string) => JSON.parse(content),
      },
    ]);

    db.startWatcher();

    const start = Date.now();
    while (Date.now() - start < 300) {
      /* busy wait */
    }

    writeFileSync(
      join(dir, "item.json"),
      JSON.stringify([{ name: "final" }]),
    );

    let events: ReturnType<typeof db.pollEvents> = [];
    const deadline = Date.now() + 5000;
    while (events.length === 0 && Date.now() < deadline) {
      events = db.pollEvents(500);
    }

    expect(events.length).toBeGreaterThanOrEqual(1);
    const actions = new Set(events.map((e) => e.action));
    expect(
      actions.has("update") || (actions.has("delete") && actions.has("insert")),
    ).toBe(true);
  });

  it("emits error events for bad extract", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "**/*.json",
        extract: (_: string, content: string) => JSON.parse(content),
      },
    ]);

    db.startWatcher();

    const start = Date.now();
    while (Date.now() - start < 300) {
      /* busy wait */
    }

    writeFileSync(join(dir, "bad.json"), "not json at all");

    let events: ReturnType<typeof db.pollEvents> = [];
    const deadline = Date.now() + 5000;
    while (events.length === 0 && Date.now() < deadline) {
      events = db.pollEvents(500);
    }

    expect(events.length).toBeGreaterThanOrEqual(1);
    expect(events[0].action).toBe("error");
    expect(events[0].error).toBeTruthy();
  });
});

describe("DirSQL ready", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-ready-"));
    mkdirSync(join(dir, "data"), { recursive: true });
    writeFileSync(
      join(dir, "data", "users.json"),
      JSON.stringify([
        { name: "Alice", age: 30 },
        { name: "Bob", age: 25 },
      ]),
    );
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("indexes files after ready", async () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
        glob: "data/users.json",
        extract: (_: string, content: string) => JSON.parse(content),
      },
    ]);

    await db.ready;
    const rows = db.query("SELECT * FROM users ORDER BY name");
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("Alice");
  });

  it("allows multiple ready awaits", async () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
        glob: "data/users.json",
        extract: (_: string, content: string) => JSON.parse(content),
      },
    ]);

    await db.ready;
    await db.ready;
    const rows = db.query("SELECT * FROM users");
    expect(rows).toHaveLength(2);
  });

  it("throws on invalid SQL after ready", async () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT)",
        glob: "data/users.json",
        extract: (_: string, content: string) => JSON.parse(content),
      },
    ]);

    await db.ready;
    expect(() => db.query("NOT VALID SQL")).toThrow();
  });
});

describe("DirSQL.fromConfig (async ready)", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-ready-config-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("loads config and awaits ready", async () => {
    mkdirSync(join(dir, "items"), { recursive: true });
    writeFileSync(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", price: 1.5 }),
    );
    writeFileSync(
      join(dir, ".dirsql.toml"),
      `[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "items/*.json"
`,
    );

    const db = DirSQL.fromConfig(join(dir, ".dirsql.toml"));
    await db.ready;
    const rows = db.query("SELECT * FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("apple");
  });
});

describe("DirSQL watch (async iterable)", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-watch-async-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("yields insert events via async iterable", async () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "**/*.json",
        extract: (_: string, content: string) => JSON.parse(content),
      },
    ]);

    await db.ready;

    const events: Array<{ action: string; table: string; row?: unknown }> = [];

    const collectPromise = (async () => {
      for await (const event of db.watch()) {
        events.push(event);
        if (events.length >= 1) break;
      }
    })();

    // Give watcher time to start
    await new Promise((r) => setTimeout(r, 300));

    writeFileSync(
      join(dir, "new_item.json"),
      JSON.stringify([{ name: "apple" }]),
    );

    // Wait with timeout
    await Promise.race([
      collectPromise,
      new Promise((_, reject) =>
        setTimeout(() => reject(new Error("Timed out")), 5000),
      ),
    ]);

    expect(events.length).toBeGreaterThanOrEqual(1);
    expect(events[0].action).toBe("insert");
    expect(events[0].table).toBe("items");
  });
});
