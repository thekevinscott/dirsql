// Integration tests for DirSQL.fromConfig — the TS mirror of
// packages/python/tests/integration/test_from_config.py and
// packages/rust/tests/from_config.rs. See bead dirsql-hh3.

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

function writeFile(path: string, content: string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
}

describe("DirSQL.fromConfig", () => {
  let dir: string;
  let configPath: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-fromconfig-"));
    configPath = join(dir, ".dirsql.toml");
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  // Basic format: JSON
  it("loads JSON files via config", () => {
    writeFile(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", price: 1.5 }),
    );
    writeFile(
      join(dir, "items", "b.json"),
      JSON.stringify({ name: "banana", price: 0.75 }),
    );
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "items/*.json"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM items ORDER BY name");
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("apple");
    expect(rows[0].price).toBeCloseTo(1.5);
    expect(rows[1].name).toBe("banana");
  });

  // Basic format: JSONL
  it("loads JSONL files via config", () => {
    writeFile(
      join(dir, "events.jsonl"),
      `${JSON.stringify({ type: "click", count: 5 })}\n${JSON.stringify({
        type: "view",
        count: 100,
      })}\n`,
    );
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE events (type TEXT, count INTEGER)"
glob = "*.jsonl"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM events ORDER BY type");
    expect(rows).toHaveLength(2);
    expect(rows[0].type).toBe("click");
    expect(rows[0].count).toBe(5);
  });

  // NDJSON alias
  it("loads NDJSON files via config", () => {
    writeFile(
      join(dir, "events.ndjson"),
      `${JSON.stringify({ type: "a" })}\n${JSON.stringify({ type: "b" })}\n`,
    );
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE events (type TEXT)"
glob = "*.ndjson"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT type FROM events ORDER BY type");
    expect(rows).toHaveLength(2);
    expect(rows[0].type).toBe("a");
    expect(rows[1].type).toBe("b");
  });

  // CSV
  it("loads CSV files via config", () => {
    writeFile(join(dir, "data.csv"), "name,count\napples,10\noranges,20\n");
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE produce (name TEXT, count TEXT)"
glob = "*.csv"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM produce ORDER BY name");
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("apples");
    expect(rows[1].name).toBe("oranges");
  });

  // TSV
  it("loads TSV files via config", () => {
    writeFile(join(dir, "data.tsv"), "name\tcount\napples\t10\noranges\t20\n");
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE produce (name TEXT, count TEXT)"
glob = "*.tsv"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM produce ORDER BY name");
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("apples");
    expect(rows[1].name).toBe("oranges");
  });

  // TOML
  it("loads TOML files via config", () => {
    writeFile(join(dir, "settings.toml"), `name = "root"\nvalue = "42"\n`);
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE settings (name TEXT, value TEXT)"
glob = "*.toml"
`,
    );

    // The config file itself also matches "*.toml", so it must be excluded.
    // Put it in the root and scan only a subdir instead:
    // Simpler approach — restructure.
    rmSync(configPath, { force: true });
    const subdir = join(dir, "data");
    mkdirSync(subdir, { recursive: true });
    writeFile(join(subdir, "a.toml"), `name = "root"\nvalue = "42"\n`);
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE settings (name TEXT, value TEXT)"
glob = "data/*.toml"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM settings");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("root");
    expect(rows[0].value).toBe("42");
  });

  // YAML
  it("loads YAML files via config (.yaml)", () => {
    writeFile(join(dir, "data", "a.yaml"), "name: apple\ncolor: red\n");
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE items (name TEXT, color TEXT)"
glob = "data/*.yaml"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("apple");
    expect(rows[0].color).toBe("red");
  });

  // YAML .yml alias
  it("loads YAML files via config (.yml)", () => {
    writeFile(join(dir, "data", "a.yml"), "name: banana\n");
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "data/*.yml"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("banana");
  });

  // Markdown + frontmatter
  it("loads markdown with frontmatter via config", () => {
    writeFile(
      join(dir, "posts", "hello.md"),
      "---\ntitle: Hello\nauthor: Alice\n---\nThe body text here.\n",
    );
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT, body TEXT)"
glob = "posts/*.md"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM posts");
    expect(rows).toHaveLength(1);
    expect(rows[0].title).toBe("Hello");
    expect(rows[0].author).toBe("Alice");
    expect(String(rows[0].body ?? "")).toContain("The body text");
  });

  // Path captures
  it("injects path captures into rows", () => {
    writeFile(
      join(dir, "comments", "thread-1", "index.jsonl"),
      `${JSON.stringify({ body: "hello", author: "alice" })}\n`,
    );
    writeFile(
      join(dir, "comments", "thread-2", "index.jsonl"),
      `${JSON.stringify({ body: "world", author: "bob" })}\n`,
    );
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, body TEXT, author TEXT)"
glob = "comments/{thread_id}/index.jsonl"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM comments ORDER BY thread_id");
    expect(rows).toHaveLength(2);
    expect(rows[0].thread_id).toBe("thread-1");
    expect(rows[0].body).toBe("hello");
    expect(rows[1].thread_id).toBe("thread-2");
  });

  // Column mapping
  it("applies column mapping", () => {
    writeFile(
      join(dir, "people", "alice.json"),
      JSON.stringify({ metadata: { author: { name: "Alice" } }, age: 30 }),
    );
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE people (display_name TEXT, age INTEGER)"
glob = "people/*.json"

[table.columns]
display_name = "metadata.author.name"
age = "age"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM people");
    expect(rows).toHaveLength(1);
    expect(rows[0].display_name).toBe("Alice");
    expect(rows[0].age).toBe(30);
  });

  // each
  it("uses each to navigate into arrays", () => {
    writeFile(
      join(dir, "catalog.json"),
      JSON.stringify({
        data: {
          items: [
            { name: "widget", price: 9.99 },
            { name: "gadget", price: 19.99 },
          ],
        },
      }),
    );
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "catalog.json"
each = "data.items"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM items ORDER BY name");
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("gadget");
    expect(rows[1].name).toBe("widget");
  });

  // ignore
  it("respects ignore patterns", () => {
    writeFile(join(dir, "data", "good.json"), JSON.stringify({ val: 1 }));
    writeFile(
      join(dir, "data", "node_modules", "bad.json"),
      JSON.stringify({ val: 2 }),
    );
    writeFile(
      configPath,
      `
[dirsql]
ignore = ["**/node_modules/**"]

[[table]]
ddl = "CREATE TABLE items (val INTEGER)"
glob = "data/**/*.json"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].val).toBe(1);
  });

  // Multiple tables
  it("loads multiple tables", () => {
    writeFile(
      join(dir, "posts", "hello.json"),
      JSON.stringify({ title: "Hello" }),
    );
    writeFile(
      join(dir, "authors", "alice.json"),
      JSON.stringify({ name: "Alice" }),
    );
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE posts (title TEXT)"
glob = "posts/*.json"

[[table]]
ddl = "CREATE TABLE authors (name TEXT)"
glob = "authors/*.json"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const posts = db.query("SELECT * FROM posts");
    const authors = db.query("SELECT * FROM authors");
    expect(posts).toHaveLength(1);
    expect(authors).toHaveLength(1);
    expect(posts[0].title).toBe("Hello");
    expect(authors[0].name).toBe("Alice");
  });

  // Explicit format override
  it("uses explicit format override", () => {
    writeFile(join(dir, "data.txt"), "name,val\nfoo,1\n");
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE t (name TEXT, val TEXT)"
glob = "*.txt"
format = "csv"
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT * FROM t");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("foo");
    expect(rows[0].val).toBe("1");
  });

  // Strict = true (passing)
  it("allows rows with exact keys when strict = true in config", () => {
    writeFile(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", color: "red" }),
    );
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE items (name TEXT, color TEXT)"
glob = "items/*.json"
strict = true
`,
    );

    const db = DirSQL.fromConfig(configPath);
    const rows = db.query("SELECT name, color FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("apple");
    expect(rows[0].color).toBe("red");
  });

  // Strict = true (rejecting)
  it("rejects rows with extra keys when strict = true in config", () => {
    writeFile(
      join(dir, "items", "a.json"),
      JSON.stringify({ name: "apple", color: "red" }),
    );
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "items/*.json"
strict = true
`,
    );

    expect(() => DirSQL.fromConfig(configPath)).toThrow();
  });

  // Error: missing config file
  it("throws when config file is missing", () => {
    expect(() => DirSQL.fromConfig(join(dir, "nonexistent.toml"))).toThrow();
  });

  // Error: invalid TOML
  it("throws on invalid TOML", () => {
    writeFile(configPath, "this is not valid [[[");
    expect(() => DirSQL.fromConfig(configPath)).toThrow();
  });

  // Error: missing DDL
  it("throws when a table entry is missing ddl", () => {
    writeFile(
      configPath,
      `
[[table]]
glob = "*.json"
`,
    );
    expect(() => DirSQL.fromConfig(configPath)).toThrow();
  });

  // Error: unsupported format
  it("throws when format cannot be inferred and none given", () => {
    writeFile(join(dir, "data.dat"), "some data");
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE t (x TEXT)"
glob = "*.dat"
`,
    );
    expect(() => DirSQL.fromConfig(configPath)).toThrow();
  });
});
