// Binding-tier tests (real core, real fs) that mirror the code examples in
// the docs (#294 test parity).
//
// The Python and Rust SDKs each carry a docs-examples suite
// (packages/python/tests/binding/docs_examples_test.py,
// packages/rust/tests/docs_examples.rs); this is the TypeScript mirror. Each
// test is named for the doc page and section it verifies — if a doc example
// changes and these tests break, the docs need updating (or vice versa).
//
// Watching-guide event examples live in watch.test.ts alongside the rest of
// the `watch()` iterator coverage.

import { readFileSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL, Table, type TableDef } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

let dir: string;

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), "dirsql-docs-"));
});

afterEach(async () => {
  await rm(dir, { recursive: true, force: true });
});

const readJson = (filePath: string) => [
  JSON.parse(readFileSync(filePath, "utf8")),
];

/** Set up the blog directory structure used in getting-started.md. */
async function blogDir(root: string): Promise<void> {
  await mkdir(join(root, "posts"), { recursive: true });
  await mkdir(join(root, "authors"), { recursive: true });
  await writeFile(
    join(root, "posts", "hello.json"),
    JSON.stringify({ title: "Hello World", author: "alice" }),
  );
  await writeFile(
    join(root, "posts", "second.json"),
    JSON.stringify({ title: "Second Post", author: "bob" }),
  );
  await writeFile(
    join(root, "authors", "alice.json"),
    JSON.stringify({ id: "alice", name: "Alice" }),
  );
  await writeFile(
    join(root, "authors", "bob.json"),
    JSON.stringify({ id: "bob", name: "Bob" }),
  );
}

/** The table definitions from the getting-started example. */
function blogTables(): TableDef[] {
  return [
    {
      ddl: "CREATE TABLE posts (title TEXT, author TEXT)",
      glob: "posts/*.json",
      extract: readJson,
    },
    {
      ddl: "CREATE TABLE authors (id TEXT, name TEXT)",
      glob: "authors/*.json",
      extract: readJson,
    },
  ];
}

// ---------------------------------------------------------------------------
// getting-started.md
// ---------------------------------------------------------------------------

describe("getting started", () => {
  it("matches getting-started query all posts", async () => {
    await blogDir(dir);
    const db = new DirSQL({ root: dir, tables: blogTables() });

    const posts = await db.query("SELECT * FROM posts");
    const titles = posts.map((p) => p.title).sort();
    expect(titles).toEqual(["Hello World", "Second Post"]);
  });

  it("matches getting-started join example", async () => {
    await blogDir(dir);
    const db = new DirSQL({ root: dir, tables: blogTables() });

    const results = await db.query(
      "SELECT posts.title, authors.name FROM posts JOIN authors ON posts.author = authors.id",
    );
    const resultMap = Object.fromEntries(results.map((r) => [r.title, r.name]));
    expect(resultMap).toEqual({
      "Hello World": "Alice",
      "Second Post": "Bob",
    });
  });
});

// ---------------------------------------------------------------------------
// guide/tables.md
// ---------------------------------------------------------------------------

describe("tables guide", () => {
  it("matches tables guide single-object JSON", async () => {
    await mkdir(join(dir, "data"), { recursive: true });
    await writeFile(
      join(dir, "data", "item.json"),
      JSON.stringify({ name: "widget", value: 42 }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT, value INTEGER)",
          glob: "data/*.json",
          extract: readJson,
        },
      ],
    });

    const results = await db.query("SELECT * FROM items");
    expect(results).toHaveLength(1);
    expect(results[0].name).toBe("widget");
    expect(results[0].value).toBe(42);
  });

  it("matches tables guide JSONL extraction", async () => {
    await mkdir(join(dir, "comments", "abc"), { recursive: true });
    await writeFile(
      join(dir, "comments", "abc", "index.jsonl"),
      `${JSON.stringify({ body: "first", author: "alice" })}\n${JSON.stringify({
        body: "second",
        author: "bob",
      })}\n`,
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE comments (body TEXT, author TEXT)",
          glob: "comments/**/index.jsonl",
          extract: (filePath: string) =>
            readFileSync(filePath, "utf8")
              .split("\n")
              .filter((line) => line.length > 0)
              .map((line) => JSON.parse(line)),
        },
      ],
    });

    const results = await db.query("SELECT * FROM comments");
    expect(results).toHaveLength(2);
    expect(results.map((r) => r.author).sort()).toEqual(["alice", "bob"]);
  });

  it("matches tables guide derive-from-path", async () => {
    await mkdir(join(dir, "comments", "abc"), { recursive: true });
    await writeFile(
      join(dir, "comments", "abc", "index.jsonl"),
      `${JSON.stringify({ body: "hello" })}\n`,
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE comments (id TEXT, body TEXT)",
          glob: "comments/**/index.jsonl",
          extract: (filePath: string) =>
            readFileSync(filePath, "utf8")
              .split("\n")
              .filter((line) => line.length > 0)
              .map((line) => ({
                // Derive the id from the parent directory name.
                id: filePath.split("/").slice(-2, -1)[0],
                body: JSON.parse(line).body,
              })),
        },
      ],
    });

    const results = await db.query("SELECT * FROM comments");
    expect(results).toHaveLength(1);
    expect(results[0].id).toBe("abc");
    expect(results[0].body).toBe("hello");
  });

  it("matches tables guide skip-draft-files", async () => {
    await writeFile(
      join(dir, "draft.json"),
      JSON.stringify({ title: "Draft Post", draft: true }),
    );
    await writeFile(
      join(dir, "published.json"),
      JSON.stringify({ title: "Published Post", draft: false }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE posts (title TEXT)",
          glob: "*.json",
          extract: (filePath: string) => {
            const data = JSON.parse(readFileSync(filePath, "utf8"));
            // Conditionally skip files by returning [].
            return data.draft ? [] : [{ title: data.title }];
          },
        },
      ],
    });

    const results = await db.query("SELECT * FROM posts");
    expect(results).toHaveLength(1);
    expect(results[0].title).toBe("Published Post");
  });

  it("matches tables guide multiple tables", async () => {
    await mkdir(join(dir, "posts"), { recursive: true });
    await mkdir(join(dir, "authors"), { recursive: true });
    await writeFile(
      join(dir, "posts", "hello.json"),
      JSON.stringify({ title: "Hello World", author_id: "1" }),
    );
    await writeFile(
      join(dir, "authors", "alice.json"),
      JSON.stringify({ id: "1", name: "Alice" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE posts (title TEXT, author_id TEXT)",
          glob: "posts/*.json",
          extract: readJson,
        },
        {
          ddl: "CREATE TABLE authors (id TEXT, name TEXT)",
          glob: "authors/*.json",
          extract: readJson,
        },
      ],
    });

    expect(await db.query("SELECT * FROM posts")).toHaveLength(1);
    expect(await db.query("SELECT * FROM authors")).toHaveLength(1);
  });

  it("matches tables guide ignore patterns", async () => {
    await mkdir(join(dir, "data"), { recursive: true });
    await mkdir(join(dir, "node_modules"), { recursive: true });
    await writeFile(
      join(dir, "data", "item.json"),
      JSON.stringify({ name: "real" }),
    );
    await writeFile(
      join(dir, "node_modules", "dep.json"),
      JSON.stringify({ name: "ignored" }),
    );

    const db = new DirSQL({
      root: dir,
      ignore: ["**/node_modules/**"],
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: readJson,
        },
      ],
    });

    const results = await db.query("SELECT * FROM items");
    expect(results).toHaveLength(1);
    expect(results[0].name).toBe("real");
  });

  it("matches tables guide typed columns", async () => {
    await mkdir(join(dir, "data"), { recursive: true });
    await writeFile(
      join(dir, "data", "metric.json"),
      JSON.stringify({ name: "cpu", value: 0.85, count: 100 }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE metrics (name TEXT, value REAL, count INTEGER)",
          glob: "data/*.json",
          extract: readJson,
        },
      ],
    });

    const results = await db.query("SELECT * FROM metrics");
    expect(results).toHaveLength(1);
    expect(results[0].name).toBe("cpu");
    expect(results[0].value).toBeCloseTo(0.85);
    expect(results[0].count).toBe(100);
  });

  it("matches tables guide constraints", async () => {
    await mkdir(join(dir, "data"), { recursive: true });
    await writeFile(
      join(dir, "data", "item.json"),
      JSON.stringify({ id: "abc", name: "Widget" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (id TEXT PRIMARY KEY, name TEXT NOT NULL)",
          glob: "data/*.json",
          extract: readJson,
        },
      ],
    });

    const results = await db.query("SELECT * FROM items");
    expect(results).toHaveLength(1);
    expect(results[0].id).toBe("abc");
    expect(results[0].name).toBe("Widget");
  });

  // Docs (guide/tables.md "Supported value types"): string -> TEXT,
  // integer number -> INTEGER, fractional number -> REAL,
  // boolean -> INTEGER (0/1), null -> NULL. (`bytes -> BLOB` is documented
  // for Python only; the TS binding has no Buffer -> BLOB mapping — a
  // tracked parity gap, see PARITY.md.)
  it("matches tables guide value types", async () => {
    await writeFile(
      join(dir, "item.json"),
      JSON.stringify({
        text_val: "hello",
        int_val: 42,
        float_val: 3.14,
        bool_val: true,
        null_val: null,
      }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (text_val TEXT, int_val INTEGER, float_val REAL, bool_val INTEGER, null_val TEXT)",
          glob: "*.json",
          extract: readJson,
        },
      ],
    });

    const results = await db.query("SELECT * FROM items");
    expect(results).toHaveLength(1);
    const row = results[0];
    expect(row.text_val).toBe("hello");
    expect(row.int_val).toBe(42);
    expect(row.float_val).toBeCloseTo(3.14);
    expect(row.bool_val).toBe(1); // boolean -> INTEGER 0/1
    expect(row.null_val).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// guide/querying.md
// ---------------------------------------------------------------------------

describe("querying guide", () => {
  it("matches querying guide select all", async () => {
    await blogDir(dir);
    const db = new DirSQL({ root: dir, tables: blogTables() });

    expect(await db.query("SELECT * FROM posts")).toHaveLength(2);
  });

  it("matches querying guide WHERE filter", async () => {
    await blogDir(dir);
    const db = new DirSQL({ root: dir, tables: blogTables() });

    const results = await db.query(
      "SELECT * FROM posts WHERE author = 'alice'",
    );
    expect(results).toHaveLength(1);
    expect(results[0].title).toBe("Hello World");
  });

  it("matches querying guide aggregation", async () => {
    await blogDir(dir);
    const db = new DirSQL({ root: dir, tables: blogTables() });

    const results = await db.query(
      "SELECT author, COUNT(*) as n FROM posts GROUP BY author",
    );
    expect(results).toHaveLength(2);
    const countMap = Object.fromEntries(results.map((r) => [r.author, r.n]));
    expect(countMap).toEqual({ alice: 1, bob: 1 });
  });

  it("matches querying guide join", async () => {
    await blogDir(dir);
    const db = new DirSQL({ root: dir, tables: blogTables() });

    const results = await db.query(
      "SELECT posts.title, authors.name FROM posts JOIN authors ON posts.author = authors.id",
    );
    expect(results).toHaveLength(2);
  });

  it("matches querying guide return format", async () => {
    await blogDir(dir);
    const db = new DirSQL({ root: dir, tables: blogTables() });

    const results = await db.query("SELECT title, author FROM posts");
    expect(Array.isArray(results)).toBe(true);
    for (const r of results) {
      expect(typeof r).toBe("object");
      expect(r).toHaveProperty("title");
      expect(r).toHaveProperty("author");
    }
  });

  it("matches querying guide internal columns excluded", async () => {
    await blogDir(dir);
    const db = new DirSQL({ root: dir, tables: blogTables() });

    const results = await db.query("SELECT * FROM posts LIMIT 1");
    expect(results[0]).not.toHaveProperty("_dirsql_file_path");
    expect(results[0]).not.toHaveProperty("_dirsql_row_index");
  });

  it("matches querying guide error handling", async () => {
    await blogDir(dir);
    const db = new DirSQL({ root: dir, tables: blogTables() });

    await expect(db.query("NOT VALID SQL")).rejects.toThrow();
  });

  it("matches querying guide empty results", async () => {
    await blogDir(dir);
    const db = new DirSQL({ root: dir, tables: blogTables() });

    const results = await db.query(
      "SELECT * FROM posts WHERE author = 'nobody'",
    );
    expect(results).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// guide/async.md
// ---------------------------------------------------------------------------

describe("async guide", () => {
  it("matches async guide basic usage", async () => {
    await mkdir(join(dir, "data"), { recursive: true });
    await writeFile(
      join(dir, "data", "a.json"),
      JSON.stringify({ name: "low", value: 5 }),
    );
    await writeFile(
      join(dir, "data", "b.json"),
      JSON.stringify({ name: "high", value: 15 }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT, value INTEGER)",
          glob: "data/*.json",
          extract: readJson,
        },
      ],
    });
    await db.ready;

    const results = await db.query("SELECT * FROM items WHERE value > 10");
    expect(results).toHaveLength(1);
    expect(results[0].name).toBe("high");
    expect(results[0].value).toBe(15);
  });

  it("matches async guide ready idempotent", async () => {
    await mkdir(join(dir, "data"), { recursive: true });
    await writeFile(
      join(dir, "data", "item.json"),
      JSON.stringify({ name: "test", value: 1 }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT, value INTEGER)",
          glob: "data/*.json",
          extract: readJson,
        },
      ],
    });
    await db.ready;
    await db.ready;

    expect(await db.query("SELECT * FROM items")).toHaveLength(1);
  });

  it("matches async guide count query", async () => {
    await mkdir(join(dir, "data"), { recursive: true });
    await writeFile(
      join(dir, "data", "a.json"),
      JSON.stringify({ name: "one", value: 1 }),
    );
    await writeFile(
      join(dir, "data", "b.json"),
      JSON.stringify({ name: "two", value: 2 }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT, value INTEGER)",
          glob: "data/*.json",
          extract: readJson,
        },
      ],
    });

    const results = await db.query("SELECT COUNT(*) as n FROM items");
    expect(results).toEqual([{ n: 2 }]);
  });
});

// ---------------------------------------------------------------------------
// api/index.md
// ---------------------------------------------------------------------------

describe("api reference", () => {
  it("matches api reference DirSQL constructor", async () => {
    await blogDir(dir);
    const db = new DirSQL({ root: dir, tables: blogTables() });
    await db.ready;
    expect(await db.query("SELECT * FROM posts")).toHaveLength(2);
  });

  it("matches api reference DirSQL query", async () => {
    await blogDir(dir);
    const db = new DirSQL({ root: dir, tables: blogTables() });

    const results = await db.query("SELECT title FROM posts");
    expect(Array.isArray(results)).toBe(true);
    for (const r of results) {
      expect(typeof r).toBe("object");
    }
  });

  it("matches api reference Table attributes", () => {
    const table = new Table({
      ddl: "CREATE TABLE items (name TEXT)",
      glob: "**/*.json",
      extract: readJson,
    });
    expect(table.ddl).toBe("CREATE TABLE items (name TEXT)");
    expect(table.glob).toBe("**/*.json");
  });

  it("matches api reference ready-then-query flow", async () => {
    await mkdir(join(dir, "data"), { recursive: true });
    await writeFile(
      join(dir, "data", "item.json"),
      JSON.stringify({ name: "test" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "data/*.json",
          extract: readJson,
        },
      ],
    });
    await db.ready;
    expect(await db.query("SELECT * FROM items")).toHaveLength(1);
  });
});
