/**
 * Integration tests for the DirSQL TypeScript SDK.
 * Mirrors the Python integration test suite for parity.
 */

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import { DirSQL } from "../../src/index.js";
import type { Table, RowEvent } from "../../src/index.js";

function makeTmpDir(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), "dirsql-test-"));
}

function makeJsonlDir(): string {
  const dir = makeTmpDir();
  fs.mkdirSync(path.join(dir, "comments", "abc"), { recursive: true });
  fs.mkdirSync(path.join(dir, "comments", "def"), { recursive: true });

  fs.writeFileSync(
    path.join(dir, "comments", "abc", "index.jsonl"),
    JSON.stringify({ body: "first comment", author: "alice" }) +
      "\n" +
      JSON.stringify({ body: "second comment", author: "bob" }) +
      "\n",
  );

  fs.writeFileSync(
    path.join(dir, "comments", "def", "index.jsonl"),
    JSON.stringify({ body: "another comment", author: "carol" }) + "\n",
  );

  return dir;
}

const commentTable: Table = {
  ddl: "CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
  glob: "comments/**/index.jsonl",
  extract: (filePath, content) =>
    content
      .trim()
      .split("\n")
      .filter((line) => line.length > 0)
      .map((line) => {
        const row = JSON.parse(line);
        const id = path.basename(path.dirname(filePath));
        return { id, body: row.body, author: row.author };
      }),
};

describe("DirSQL", () => {
  let tmpDir: string;

  afterEach(() => {
    if (tmpDir) {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  describe("init", () => {
    it("creates instance with tables", async () => {
      tmpDir = makeJsonlDir();
      const db = new DirSQL(tmpDir, [commentTable]);
      await db.ready;
      expect(db).toBeDefined();
    });

    it("accepts ignore patterns", async () => {
      tmpDir = makeJsonlDir();
      const db = new DirSQL(tmpDir, [commentTable], {
        ignore: ["**/def/**"],
      });
      await db.ready;
      const results = db.query("SELECT DISTINCT id FROM comments");
      const ids = new Set(results.map((r) => r.id));
      expect(ids).toEqual(new Set(["abc"]));
    });
  });

  describe("query", () => {
    it("returns all rows", async () => {
      tmpDir = makeJsonlDir();
      const db = new DirSQL(tmpDir, [commentTable]);
      await db.ready;
      const results = db.query("SELECT * FROM comments");
      expect(results).toHaveLength(3);
    });

    it("returns dicts with column names", async () => {
      tmpDir = makeJsonlDir();
      const db = new DirSQL(tmpDir, [commentTable]);
      await db.ready;
      const results = db.query(
        "SELECT author FROM comments WHERE body = 'first comment'",
      );
      expect(results).toHaveLength(1);
      expect(results[0].author).toBe("alice");
    });

    it("filters with WHERE clause", async () => {
      tmpDir = makeJsonlDir();
      const db = new DirSQL(tmpDir, [commentTable]);
      await db.ready;
      const results = db.query("SELECT * FROM comments WHERE id = 'abc'");
      expect(results).toHaveLength(2);
      expect(results.every((r) => r.id === "abc")).toBe(true);
    });

    it("excludes internal tracking columns", async () => {
      tmpDir = makeJsonlDir();
      const db = new DirSQL(tmpDir, [commentTable]);
      await db.ready;
      const results = db.query("SELECT * FROM comments LIMIT 1");
      expect(results).toHaveLength(1);
      expect(results[0]).not.toHaveProperty("_dirsql_file_path");
      expect(results[0]).not.toHaveProperty("_dirsql_row_index");
    });

    it("handles integer values", async () => {
      tmpDir = makeTmpDir();
      fs.mkdirSync(path.join(tmpDir, "data"), { recursive: true });
      fs.writeFileSync(
        path.join(tmpDir, "data", "counts.json"),
        JSON.stringify({ name: "apples", count: 42 }),
      );

      const db = new DirSQL(tmpDir, [
        {
          ddl: "CREATE TABLE items (name TEXT, count INTEGER)",
          glob: "data/*.json",
          extract: (_p, content) => [JSON.parse(content)],
        },
      ]);
      await db.ready;

      const results = db.query("SELECT * FROM items");
      expect(results).toHaveLength(1);
      expect(results[0].name).toBe("apples");
      expect(results[0].count).toBe(42);
    });
  });

  describe("multiple tables", () => {
    it("supports multiple table definitions", async () => {
      tmpDir = makeTmpDir();
      fs.mkdirSync(path.join(tmpDir, "posts"), { recursive: true });
      fs.mkdirSync(path.join(tmpDir, "authors"), { recursive: true });

      fs.writeFileSync(
        path.join(tmpDir, "posts", "hello.json"),
        JSON.stringify({ title: "Hello World", author_id: "1" }),
      );
      fs.writeFileSync(
        path.join(tmpDir, "authors", "alice.json"),
        JSON.stringify({ id: "1", name: "Alice" }),
      );

      const db = new DirSQL(tmpDir, [
        {
          ddl: "CREATE TABLE posts (title TEXT, author_id TEXT)",
          glob: "posts/*.json",
          extract: (_p, content) => [JSON.parse(content)],
        },
        {
          ddl: "CREATE TABLE authors (id TEXT, name TEXT)",
          glob: "authors/*.json",
          extract: (_p, content) => [JSON.parse(content)],
        },
      ]);
      await db.ready;

      const posts = db.query("SELECT * FROM posts");
      const authors = db.query("SELECT * FROM authors");
      expect(posts).toHaveLength(1);
      expect(authors).toHaveLength(1);
      expect(posts[0].title).toBe("Hello World");
      expect(authors[0].name).toBe("Alice");
    });

    it("supports joins across tables", async () => {
      tmpDir = makeTmpDir();
      fs.mkdirSync(path.join(tmpDir, "posts"), { recursive: true });
      fs.mkdirSync(path.join(tmpDir, "authors"), { recursive: true });

      fs.writeFileSync(
        path.join(tmpDir, "posts", "hello.json"),
        JSON.stringify({ title: "Hello World", author_id: "1" }),
      );
      fs.writeFileSync(
        path.join(tmpDir, "authors", "alice.json"),
        JSON.stringify({ id: "1", name: "Alice" }),
      );

      const db = new DirSQL(tmpDir, [
        {
          ddl: "CREATE TABLE posts (title TEXT, author_id TEXT)",
          glob: "posts/*.json",
          extract: (_p, content) => [JSON.parse(content)],
        },
        {
          ddl: "CREATE TABLE authors (id TEXT, name TEXT)",
          glob: "authors/*.json",
          extract: (_p, content) => [JSON.parse(content)],
        },
      ]);
      await db.ready;

      const results = db.query(
        "SELECT posts.title, authors.name FROM posts JOIN authors ON posts.author_id = authors.id",
      );
      expect(results).toHaveLength(1);
      expect(results[0].title).toBe("Hello World");
      expect(results[0].name).toBe("Alice");
    });
  });

  describe("error handling", () => {
    it("throws on invalid SQL", async () => {
      tmpDir = makeJsonlDir();
      const db = new DirSQL(tmpDir, [commentTable]);
      await db.ready;
      expect(() => db.query("NOT VALID SQL")).toThrow();
    });

    it("throws on invalid DDL", async () => {
      tmpDir = makeTmpDir();
      const db = new DirSQL(tmpDir, [
        {
          ddl: "NOT A CREATE TABLE",
          glob: "*.json",
          extract: () => [],
        },
      ]);
      await expect(db.ready).rejects.toThrow();
    });

    it("handles empty directory", async () => {
      tmpDir = makeTmpDir();
      const db = new DirSQL(tmpDir, [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: (_p, content) => [JSON.parse(content)],
        },
      ]);
      await db.ready;
      const results = db.query("SELECT * FROM items");
      expect(results).toHaveLength(0);
    });

    it("handles extract returning empty list", async () => {
      tmpDir = makeTmpDir();
      fs.writeFileSync(
        path.join(tmpDir, "skip.json"),
        JSON.stringify({ ignore: true }),
      );

      const db = new DirSQL(tmpDir, [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: () => [],
        },
      ]);
      await db.ready;
      const results = db.query("SELECT * FROM items");
      expect(results).toHaveLength(0);
    });
  });

  describe("extract receives path and content", () => {
    it("passes relative path and string content", async () => {
      tmpDir = makeTmpDir();
      fs.writeFileSync(
        path.join(tmpDir, "test.json"),
        JSON.stringify({ val: 1 }),
      );

      const captured: { path?: string; content?: string } = {};

      const db = new DirSQL(tmpDir, [
        {
          ddl: "CREATE TABLE t (val INTEGER)",
          glob: "*.json",
          extract: (p, content) => {
            captured.path = p;
            captured.content = content;
            return [{ val: 1 }];
          },
        },
      ]);
      await db.ready;
      db.query("SELECT * FROM t");

      expect(captured.path).toBe("test.json");
      expect(captured.content).toContain('"val"');
    });
  });

  describe("watch", () => {
    it("emits insert events for new files", async () => {
      tmpDir = makeTmpDir();
      const db = new DirSQL(tmpDir, [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: (_p, content) => [JSON.parse(content)],
        },
      ]);
      await db.ready;

      const events: RowEvent[] = [];
      const iter = db.watch()[Symbol.asyncIterator]();

      // Give the watcher time to start
      await new Promise((r) => setTimeout(r, 300));

      // Create a new file
      fs.writeFileSync(
        path.join(tmpDir, "new_item.json"),
        JSON.stringify({ name: "apple" }),
      );

      // Wait for the event
      const result = await Promise.race([
        iter.next(),
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error("Timeout")), 5000),
        ),
      ]);

      await iter.return!();

      expect(result.done).toBe(false);
      expect(result.value).toMatchObject({
        action: "insert",
        table: "items",
      });
      expect((result.value as any).row.name).toBe("apple");
    });

    it("emits delete events for removed files", async () => {
      tmpDir = makeTmpDir();
      fs.writeFileSync(
        path.join(tmpDir, "doomed.json"),
        JSON.stringify({ name: "doomed" }),
      );

      const db = new DirSQL(tmpDir, [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: (_p, content) => [JSON.parse(content)],
        },
      ]);
      await db.ready;

      const results = db.query("SELECT * FROM items");
      expect(results).toHaveLength(1);

      const iter = db.watch()[Symbol.asyncIterator]();
      await new Promise((r) => setTimeout(r, 300));

      fs.unlinkSync(path.join(tmpDir, "doomed.json"));

      const result = await Promise.race([
        iter.next(),
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error("Timeout")), 5000),
        ),
      ]);

      await iter.return!();

      expect(result.done).toBe(false);
      expect(result.value).toMatchObject({
        action: "delete",
        table: "items",
      });

      // DB should reflect deletion
      const afterResults = db.query("SELECT * FROM items");
      expect(afterResults).toHaveLength(0);
    });

    it("emits error events for bad extract", async () => {
      tmpDir = makeTmpDir();
      const db = new DirSQL(tmpDir, [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: (_p, content) => [JSON.parse(content)],
        },
      ]);
      await db.ready;

      const iter = db.watch()[Symbol.asyncIterator]();
      await new Promise((r) => setTimeout(r, 300));

      // Create a file with invalid JSON
      fs.writeFileSync(path.join(tmpDir, "bad.json"), "not json at all");

      const result = await Promise.race([
        iter.next(),
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error("Timeout")), 5000),
        ),
      ]);

      await iter.return!();

      expect(result.done).toBe(false);
      expect(result.value).toMatchObject({ action: "error" });
      expect((result.value as any).error).toBeTruthy();
    });

    it("updates db on file changes", async () => {
      tmpDir = makeTmpDir();
      const db = new DirSQL(tmpDir, [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: (_p, content) => [JSON.parse(content)],
        },
      ]);
      await db.ready;

      expect(db.query("SELECT * FROM items")).toHaveLength(0);

      const iter = db.watch()[Symbol.asyncIterator]();
      await new Promise((r) => setTimeout(r, 300));

      fs.writeFileSync(
        path.join(tmpDir, "new.json"),
        JSON.stringify({ name: "added" }),
      );

      const result = await Promise.race([
        iter.next(),
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error("Timeout")), 5000),
        ),
      ]);

      await iter.return!();

      const afterResults = db.query("SELECT * FROM items");
      expect(afterResults).toHaveLength(1);
      expect(afterResults[0].name).toBe("added");
    });
  });
});
